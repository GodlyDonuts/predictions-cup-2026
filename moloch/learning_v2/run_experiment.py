"""Finite, provenance-pinned development PPO run; no HTTP or live activation.

The entire selected source interval is replayed on the exchange clock. A fixed
receipt/cancel drain is common to all candidates, including WAIT. Missing marks
remain missing: incomplete final accounting is a failed evaluation, never a
zero/positive return. This first runner does not certify tournament outcomes.
"""
import argparse
from collections import Counter
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import random
import time
import numpy as np
import torch
from .account import Limits, UNIT
from .clock import BudgetConfig
from .episodes import Tape, CONTRACT_FEATURES, GLOBAL_FEATURES
from .policy import RecurrentPolicy, FAMILIES
from .reference_env import Mechanics
from .train_inner import PPOConfig, PPOTrainer, Trajectory, fingerprint
from .inventory_curriculum import seed_inventory
from .excess_reward import MatchedHoldReward


def sha(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write_json(path, obj):
    with Path(path).open('x',encoding='utf-8') as out:
        json.dump(obj,out,indent=2,allow_nan=False); out.write('\n')


def load_mechanics(path):
    obj = json.loads(path.read_text())
    if obj['production_authority'] is not False or obj['classification']!='conditional_development_mechanics':
        raise ValueError('RESEARCH_MECHANICS_REQUIRED')
    b = BudgetConfig(**obj['budget'])
    limits = dict(obj['limits']); limits['inherited_exchanges']=frozenset(limits.get('inherited_exchanges',[]))
    return Mechanics(b,Limits(**limits),**obj['hypotheses'])


def rollout(tape, mechanics, protocol, model, start, *, baseline=False, split_role='fit_training'):
    objective=protocol.get('training_reward_objective','marked_wealth_return')
    if objective not in ['marked_wealth_return','matched_hold_excess','realized_pnl']:raise ValueError('UNKNOWN_TRAINING_REWARD_OBJECTIVE')
    hold_reward=None;excess_observation=None;previous_realized=0
    device = next(model.parameters()).device
    length = protocol['episode_length_ms']; drain = protocol['maximum_receipt_drain_ms']
    initial = protocol['initial_cash_micros']; end = start+length; terminal = end+drain
    role_start,role_end = ((protocol['run_start_ms'],protocol['fit_end_ms']) if split_role=='fit_training' else
                          (protocol['search_start_ms'],protocol['run_start_ms']+protocol['run_duration_ms']) if split_role=='search_development' else (None,None))
    if role_start is None or start-protocol['recurrent_burnin_ms']<role_start or terminal>role_end:
        raise ValueError('EPISODE_OR_BURNIN_OUTSIDE_DECLARED_ROLE')
    env = tape.episode(mechanics,start,terminal,initial_cash_micros=initial,
                       target_wealth_micros=protocol['planning_target_wealth_micros'],
                       burnin_ms=protocol['recurrent_burnin_ms'],advance_to_start=False)
    behavior = fingerprint(model); hidden = model.initial_state(1)
    rows=[]; families=Counter(); missing=0; previous=initial; peak_committed=0.;tradable_frames=0;tradable_market_frames=0;observable_market_frames=0;begun=time.perf_counter();initial_portfolio=None
    for step,at in enumerate(range(start-protocol['recurrent_burnin_ms'],end,protocol['decision_interval_ms'])):
        env.advance(at)
        if at==start and protocol.get('initial_inventory_allocation_ppm',0):
            initial_portfolio=seed_inventory(env,protocol['initial_inventory_allocation_ppm'],protocol['initial_inventory_seed'],
                                            selection=protocol.get('initial_inventory_selection','shuffle'))
            initial=initial_portfolio['initial_marked_wealth_micros'];previous=initial;env.initial_wealth=initial
        if at>=start and hold_reward is None:hold_reward=MatchedHoldReward(env.account(),initial)
        obs=env.observe()
        if (model.contract_features,model.global_features)not in[(20,12),(CONTRACT_FEATURES,GLOBAL_FEATURES)]:
            raise ValueError('UNKNOWN_FEATURE_SEMANTICS')
        # The named legacy prefix keeps its original feature meanings. All
        # source/adapter hashes remain in the new experiment provenance.
        view={**obs,'contracts':obs['contracts'][:,:model.contract_features],
              'global_state':obs['global_state'][:model.global_features]}
        tensors={k:torch.as_tensor(view[k],device=device).unsqueeze(0) for k in ['contracts','global_state','present']}
        masks={k:torch.as_tensor(v,device=device).unsqueeze(0)for k,v in obs['masks'].items()}
        if (model.contract_features,model.global_features)==(20,12):masks.pop('target_scale',None)
        reset=torch.tensor([step==0],device=device)
        with torch.no_grad():
            row=model.step(**tensors,masks=masks,hidden=hidden,reset=reset)
        hidden=row['hidden']; action={k:v.item() for k,v in row['actions'].items()}
        if baseline or at<start: action['family']=0
        if at>=start:
            families[FAMILIES[int(action['family'])]]+=1
            tradable=int(obs['masks']['family'][:,[1,2,4]].any(-1).sum())
            tradable_frames+=int(tradable>0);tradable_market_frames+=tradable
            ref_column=7 if mechanics.valuation_source=='Kalshi'else 10
            observable_market_frames+=int(((obs['contracts'][:,17]>0)&(obs['contracts'][:,ref_column]>0)&obs['present']).sum())
            if obs['wealth'] is not None:peak_committed=max(peak_committed,float(obs['global_state'][7]+obs['global_state'][1]))
        env.act(action)
        env.advance(min(end,at+protocol['decision_interval_ms']))
        current=env.marked_wealth(); valid=current is not None and obs['wealth'] is not None
        if current is None: missing+=1; reward=0.
        else:
            reward=(current-previous)/initial; previous=current
        if objective=='matched_hold_excess':
            excess_observation=hold_reward.observe(current,env.marks())if hold_reward else None
            reward=excess_observation.normalized_increment if excess_observation else 0.
            valid=valid and excess_observation is not None and excess_observation.available
        elif objective=='realized_pnl':
            reward=(env.realized-previous_realized)/initial;previous_realized=env.realized
        # Missing intermediates are explicitly masked; any eventual mark
        # incorporates the whole unobserved interval, not an assumed flat P&L.
        rows.append({**tensors,'masks':masks,'reset':reset,'actions':row['actions'],
                     'old_logprob':row['logprob'],'old_values':row['value'],
                     'reward':reward,'learn':valid and at>=start,'elapsed':min(end,at+protocol['decision_interval_ms'])-at})
    env.advance(end)
    never_sent=[qid for qid,o in env.orders.items()if o.phase=='preflight']
    for qid in never_sent:
        env.orders.pop(qid)
        env.execution_events.append({'order':qid,'at':env.now,'kind':'not_sent','reason':'FINITE_DECISION_WINDOW_ENDED'})
    for at in range(end,terminal+1,protocol['decision_interval_ms']):
        env.advance(at)
        if env.budget.active is None:
            for o in list(env.orders.values()):
                if env.cancel(o.id): break
    env.advance(terminal)
    final=env.marked_wealth()
    unresolved=bool(env.orders or env.budget.active or env.budget.unknown)
    qualified=final is not None and not unresolved
    terminal_excess=hold_reward.observe(final,env.marks())
    if objective=='matched_hold_excess':
        qualified=qualified and terminal_excess.available
        rows[-1]['reward']+=terminal_excess.normalized_increment
    elif objective=='realized_pnl':rows[-1]['reward']+=(env.realized-previous_realized)/initial
    elif final is not None: rows[-1]['reward']+=(final-previous)/initial
    rows[-1]['elapsed']+=drain
    def stack(name): return torch.stack([r[name].detach()for r in rows])
    trajectory=Trajectory(
        **{k:stack(k)for k in ['contracts','global_state','present','reset','old_logprob','old_values']},
        masks={k:torch.stack([r['masks'][k]for r in rows])for k in rows[0]['masks']},
        actions={k:torch.stack([r['actions'][k]for r in rows])for k in rows[0]['actions']},
        rewards=torch.tensor([[r['reward']]for r in rows],device=device),
        elapsed_ms=torch.tensor([[r['elapsed']]for r in rows],device=device),
        done=torch.tensor([[i==len(rows)-1]for i in range(len(rows))],device=device),
        learn=torch.tensor([[r['learn']]for r in rows],device=device),
        target_success_labels=torch.full((len(rows),1),float('nan'),device=device),
        behavior_sha256=behavior,split_manifest_sha256=protocol['_sha256'],split_role=split_role)
    stats={'training_reward_objective':objective,'terminal_hold_wealth_micros':terminal_excess.hold_wealth_micros,
           'terminal_excess_wealth_micros':terminal_excess.excess_wealth_micros,
           'start_ms':start,'decision_end_ms':end,'terminal_ms':terminal,'decisions':length//protocol['decision_interval_ms'],
           'recurrent_burnin_frames':protocol['recurrent_burnin_ms']//protocol['decision_interval_ms'],'split_role':split_role,
           'tradable_decision_frames':tradable_frames,'tradable_market_frames':tradable_market_frames,
           'observable_market_frames':observable_market_frames,
           'rollout_wall_seconds':time.perf_counter()-begun,'baseline':baseline,'qualified_accounting':qualified,
           'initial_reference_wealth_micros':initial,'synthetic_initial_portfolio':initial_portfolio,
           'modeled_realized_return':env.realized/initial,'realized_pnl_includes_fees':True,
           'final_reference_wealth_micros':final,'reference_return':(final-initial)/initial if qualified else None,
           'missing_mark_observations':missing,'unresolved_orders':len(env.orders),
           'active_request_at_terminal':env.budget.active,'filled_shares':sum(e['quantity']for e in env.fill_events),
           'fill_receipts':len(env.fill_events),'realized_micros':env.realized,'fees_micros':env.fees,
           'terminal_cash_micros':env.cash,'terminal_owned_cost_micros':env.account().owned_cost,
           'terminal_owned_shares':sum(env.account().quantities.values()),
           'peak_cost_commitment_fraction':peak_committed,'gross_turnover_fraction':sum(e['quantity']*e['price']for e in env.fill_events)/initial,
           'families':dict(families),'never_sent_drafts_at_end':len(never_sent),
           'modeled_requests':len(env.budget.request_ids),'success_labels_available':False,
           'interpretation':'conditional_marked_capital_development_not_execution_calibration_or_competition_ROI'}
    return trajectory,stats


def main():
    p=argparse.ArgumentParser(); p.add_argument('--dataset',type=Path,required=True)
    p.add_argument('--mechanics',type=Path,required=True);p.add_argument('--protocol',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True); p.add_argument('--episodes',type=int,default=4)
    p.add_argument('--seed',type=int,default=1701);p.add_argument('--device',default='cuda');args=p.parse_args()
    if args.episodes<=0 or args.output.exists(): raise ValueError('FINITE_NEW_RUN_REQUIRED')
    args.output.mkdir(parents=True); torch.set_num_threads(4)
    random.seed(args.seed);np.random.seed(args.seed);torch.manual_seed(args.seed)
    if args.device=='cuda' and not torch.cuda.is_available(): raise ValueError('CUDA_UNAVAILABLE')
    protocol=json.loads(args.protocol.read_text());protocol['_sha256']=sha(args.protocol)
    if protocol['history_roles']!=['fit_training','search_development'] or not protocol['history_previously_inspected']:
        raise ValueError('THIS_RUNNER_IS_DEVELOPMENT_ONLY')
    mechanics=load_mechanics(args.mechanics); begun=time.perf_counter();tape=Tape(args.dataset)
    load_seconds=time.perf_counter()-begun
    if tape.manifest_sha256!=protocol['initial_dataset_manifest_sha256']: raise ValueError('DATASET_PROTOCOL_MISMATCH')
    model=RecurrentPolicy(CONTRACT_FEATURES,GLOBAL_FEATURES).to(args.device)
    trainer=PPOTrainer(model,PPOConfig(epochs=2),protocol['_sha256'])
    minimum=max(tape.times[0],protocol['run_start_ms'])+protocol['recurrent_burnin_ms']
    maximum=protocol['fit_end_ms']-protocol['episode_length_ms']-protocol['maximum_receipt_drain_ms']
    if maximum<minimum: raise ValueError('ROLE_HAS_NO_COMPLETE_EPISODE')
    starts=[random.randint(minimum,maximum)for _ in range(args.episodes)]
    source_hashes={q.name:sha(q)for q in sorted(Path(__file__).parent.glob('*.py'))if not q.name.startswith('test_')}
    plan={'seed':args.seed,'starts_ms':starts,'split_manifest_sha256':protocol['_sha256'],
          'dataset_manifest_sha256':tape.manifest_sha256,'mechanics_manifest_sha256':sha(args.mechanics),
          'source_sha256':source_hashes,'device':str(next(model.parameters()).device),
          'initial_parameter_sha256':fingerprint(model),'parameters':sum(p.numel()for p in model.parameters()),
          'load_seconds':load_seconds,'production_authority':False,'locked_test_used':False}
    write_json(args.output/'plan.json',plan); print(json.dumps({'phase':'decoded','plan':plan}),flush=True)
    results=[]; baseline_results=[]
    for i,start in enumerate(starts):
        _,base=rollout(tape,mechanics,protocol,model,start,baseline=True);baseline_results.append(base)
        t,stats=rollout(tape,mechanics,protocol,model,start); stats['episode']=i
        if stats['qualified_accounting'] and bool(t.learn.any()):
            at=time.perf_counter();stats['ppo']=trainer.update(t);stats['update_wall_seconds']=time.perf_counter()-at
        else: stats['training_skipped_reason']='UNAVAILABLE_TERMINAL_ACCOUNTING_OR_NO_LEARNABLE_OBSERVATION'
        results.append(stats);write_json(args.output/f'episode-{i:03d}.json',stats)
        print(json.dumps({'phase':'episode','result':stats}),flush=True)
    provenance={'dataset_manifest_sha256':tape.manifest_sha256,'mechanics_manifest_sha256':sha(args.mechanics),
                'evaluation_protocol_sha256':protocol['_sha256'],'action_adapter_sha256':sha(Path(__file__).with_name('episodes.py')),
                'lineage':{'id':f'ppo-seed-{args.seed}','parent':None,'operator':'fresh_ppo'},
                'classification':'inspected_history_conditional_development_only','source_sha256':source_hashes}
    checkpoint=args.output/'checkpoint.pt';checkpoint_sha=trainer.save(checkpoint,provenance)
    report={**plan,'episodes':results,'hold_baselines':baseline_results,'policy_epochs':trainer.policy_epoch,
            'training_decisions':trainer.training_decisions,'accepted_optimizer_updates':trainer.accepted_optimizer_updates,'checkpoint_sha256':checkpoint_sha,
            'final_parameter_sha256':fingerprint(model),'total_wall_seconds':time.perf_counter()-begun,
            'production_changed':False,'competition_profitability_established':False,'matched_compute_ablation_complete':False}
    write_json(args.output/'report.json',report);write_json(args.output/'COMPLETE.json',{'report_sha256':sha(args.output/'report.json'),
                                                                                   'checkpoint_sha256':checkpoint_sha})
    print(json.dumps({'phase':'complete','training_decisions':trainer.training_decisions,'checkpoint_sha256':checkpoint_sha}),flush=True)


if __name__=='__main__': main()
