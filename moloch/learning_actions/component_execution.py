"""Explicit component-debt paper execution; never an exchange account client.

Static component outcomes must be supplied from pinned causal SIG evidence.
Historical topology and lending eligibility are not inferred from prices.
This bridge models accepted-fill funding, not settlement/refund/write-off.
"""
from copy import copy,deepcopy
import hashlib,json
from pathlib import Path
import numpy as np
from .collateral_ledger import CollateralLedger,Fill
from .contract import UNIT
from .order_pages import OrderPageEpisode,OrderPageTape
from .order_policy import OrderPagePolicy
from .observation_protocol import COMPONENT_DEBT
from .full_depth import FEATURES
from .feature_cache import identity


def positions(env):
    result={}
    for(ex,side),lots in env.lots.items():
        result[ex]=result.get(ex,0)+sum(q for q,_ in lots)*(1 if side=='YES'else-1)
    return {ex:q for ex,q in result.items()if q}


def configure(env,components=(),*,scope='new-counterfactual-paper-account',other_draw_order=None):
    components=tuple(components)
    if env.api_id or env.api_orders or env.api_fills:raise ValueError('COMPONENT_CONFIGURATION_REQUIRES_FRESH_PAPER_ORDERS')
    if env.mechanics.limits.fee_per_share:raise ValueError('COMPONENT_FEE_ACCOUNTING_NOT_CALIBRATED')
    if env.collateral_oracle is not None:raise ValueError('CANNOT_DOUBLE_COUNT_LEGACY_COLLATERAL_ORACLE')
    if any(ex not in env.exchanges for c in components for ex in c.exchanges):raise ValueError('COMPONENT_EXCHANGE_OUTSIDE_PAPER_SCOPE')
    if components and(type(other_draw_order)is not tuple or set(other_draw_order)!={c.id for c in components}
        or len(other_draw_order)!=len(components)):
        raise ValueError('EXPLICIT_COMPLETE_LENDING_PRIORITY_HYPOTHESIS_REQUIRED')
    ledger=CollateralLedger(cash=env.cash,positions=positions(env),components=components,scope=scope)
    env.component_ledger=ledger;env.component_other_draw_order=other_draw_order
    env.known_components=[];env.known_resource_revisions['components']=-1


def configure_from_evidence(env,source):
    """Pinned caller evidence, not an authentication/exhaustiveness proof."""
    if source is None:return
    raw=Path(source['path']).read_bytes()
    if hashlib.sha256(raw).hexdigest()!=source['sha256']:raise ValueError('PINNED_COMPONENT_EVIDENCE_CHANGED')
    value=json.loads(raw);at=value['available_at_ms']
    if type(at)is not int or at>env.now:raise ValueError('COMPONENT_EVIDENCE_NOT_YET_AVAILABLE')
    from .collateral_ledger import Component
    components=[Component(c['id'],tuple(c['exchanges']),tuple(tuple(row)for row in c['outcomes']),
        tuple(c['relationship_ids']),value['scope'],source['sha256'],c['eligible'])for c in value['components']]
    priority=value.get('other_draw_order')
    configure(env,components,scope=value['scope'],other_draw_order=None if priority is None else tuple(priority))


def collateral_rows(env):
    ledger=env.component_ledger;floors=ledger.floors()
    return [{'componentId':c.id,'coveredExchangeIds':c.exchanges,'relationshipIds':c.relationship_ids,
        'guaranteedPayoutFloor':floors[c.id],'outstandingAdvance':ledger.debt[c.id],
        'scope':ledger.scope,'diagnostic':None if c.eligible else 'conditional_paper_lending_ineligible'}
        for c in ledger.components.values()]


def ack_economics(env,fills):
    values=[r['all_execution_economics']for r in fills];last=values[-1]
    summed=('fullNotionalCost','effectiveEntryCost','collateralSavings','collateralRepayment','redemptionCredit','netBuyingPowerImpact')
    result={k:sum(v[k]for v in values)for k in summed}
    result.update({k:last[k]for k in('componentId','guaranteedPayoutFloorAfter','outstandingAdvanceAfter')})
    own=env.component_ledger.components.get(last['componentId'])
    result['relationshipIds']=()if own is None else own.relationship_ids
    return result


def append_rows(base,rows,entities):
    if not rows:return base
    n=len(rows)
    return {**base,'contracts':np.concatenate((base['contracts'],np.stack(rows))),
        'present':np.concatenate((base['present'],np.ones(n,bool))),
        'masks':{k:np.concatenate((v,np.zeros((n,*v.shape[1:]),dtype=v.dtype)))for k,v in base['masks'].items()},
        'entities':[*base['entities'],*entities]}


class ComponentEpisode(OrderPageEpisode):
    observation_protocol=COMPONENT_DEBT

    def __init__(self,*args,**kwargs):
        super().__init__(*args,**kwargs);configure(self)

    def advance_capacity(self):
        if getattr(self,'component_ledger',None)is None:return super().advance_capacity()
        return sum(self.component_ledger.available().values())

    def buying_power(self):return max(self.cash,0)+self.advance_capacity()

    def _apply_fill(self,order,price,quantity,*,origin='crossing_book'):
        ledger=self.component_ledger
        if ledger.cash!=self.cash or {k:v for k,v in ledger.positions.items()if v}!=positions(self):
            raise ValueError('COMPONENT_FIFO_STATE_DIVERGED')
        s=order.spec;fill=Fill(s.exchange,s.side,s.action,quantity,price)
        event=f'order:{order.id}:fill:{len(self.api_fills)}'
        own=ledger.owner.get(s.exchange);priority=self.component_other_draw_order
        priority=None if priority is None else tuple(k for k in priority if k!=own)
        # Quote against isolated event storage, not a copy of the growing fill
        # history. Funding/calibration failures occur before any FIFO mutation.
        preview=copy(ledger);preview.events={}
        result=preview.apply(event,fill,other_draw_order=priority)
        if result['canonical']!=fill:raise ValueError('PLACED_ORDER_CANONICAL_SIDE_CHANGED')
        debt_before=sum(ledger.debt.values());cash_before=self.cash
        super()._apply_fill(order,price,quantity,origin=origin)
        accepted=ledger.apply(event,fill,other_draw_order=priority)
        if accepted!=result or {k:v for k,v in ledger.positions.items()if v}!=positions(self):
            raise ValueError('COMPONENT_ACCEPTED_FILL_REPLAY_DIVERGED')
        self.cash=ledger.cash
        self.api_fills[-1].update(cash_delta_micros=self.cash-cash_before,
            collateral_debt_delta_micros=sum(ledger.debt.values())-debt_before,
            all_execution_economics={k:v for k,v in result.items()if k not in('canonical','componentDraws','cashDelta','totalDebtAfter')})
        self._funding_changed()

    def _snapshot_finance(self):return super()._snapshot_finance(),deepcopy(self.component_ledger)

    def _restore_finance(self,state):
        original,self.component_ledger=state;super()._restore_finance(original)

    def observe(self):
        base=super().observe();rows=[];entities=[]
        for c in self.known_components:
            row=np.zeros(FEATURES,np.float32);scale=max(1,self.initial_wealth)
            row[:3]=[c['guaranteedPayoutFloor']/scale,c['outstandingAdvance']/scale,bool(c['diagnostic'])]
            row[8:15]=identity('component',c['componentId']);row[24]=14.
            row[25:32]=identity('component_exchanges',c['coveredExchangeIds'])
            row[33:40]=identity('component_relationships',c['relationshipIds'])
            rows.append(row);entities.append(('collateral_component',c['componentId']))
        for index,ack in enumerate(self.known_acknowledgments):
            value=ack.get('all')
            if value is None:continue
            row=np.zeros(FEATURES,np.float32);scale=max(1,self.initial_wealth)
            fields=('fullNotionalCost','effectiveEntryCost','collateralSavings','collateralRepayment',
                'redemptionCredit','netBuyingPowerImpact','guaranteedPayoutFloorAfter','outstandingAdvanceAfter')
            row[:8]=[(value[k]or 0)/scale for k in fields]
            row[8:15]=identity('component',value['componentId']);row[15]=value['componentId']is not None
            row[24]=15.;row[25:32]=identity('order_id',ack['order'])
            row[33:40]=identity('component_relationships',value['relationshipIds'])
            rows.append(row);entities.append(('all_execution',index))
        return append_rows(base,rows,entities)


class ComponentTape(OrderPageTape):episode_type=ComponentEpisode


class ComponentPolicy(OrderPagePolicy):
    architecture='tartarus-raw-api-component-debt-gru-v7'
    observation_protocol=COMPONENT_DEBT


def migrate_component_debt(parent):
    if type(parent)is not OrderPagePolicy:raise ValueError('EXACT_ORDER_PAGE_PARENT_REQUIRED_FOR_COMPONENT_MIGRATION')
    child=ComponentPolicy(parent.hidden_size).to(next(parent.parameters()).device)
    child.load_state_dict(parent.state_dict());child.global_state_encoding=getattr(parent,'global_state_encoding','raw');child.train(parent.training)
    return child,{'operator':'explicit_component_debt_execution_and_scoped_reads',
        'old_architecture':parent.architecture,'new_architecture':child.architecture,
        'all_weights_preserved_exactly':True,'optimizer_reset_required':True,
        'old_trajectories_must_not_be_reused':True,'old_scores_must_not_be_reused':True,
        'independent_test_qualification':False}
