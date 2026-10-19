# Offline v5 adapter derived from the pinned v4 rollout; baseline source SHA256 1fe75c89a3bafae2989ff8d99cf31b317c69139d63d8db5db90980124ca7eacb
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
from moloch.learning_v2.account import Limits, UNIT
from moloch.learning_v2.clock import BudgetConfig
from moloch.learning_v2.episodes import Tape, CONTRACT_FEATURES, GLOBAL_FEATURES
from moloch.learning_v2.policy import RecurrentPolicy, FAMILIES
from moloch.learning_v2.reference_env import Mechanics
from moloch.learning_v2.train_inner import PPOConfig, PPOTrainer, Trajectory, fingerprint
from moloch.learning_v2.inventory_curriculum import seed_inventory
from moloch.learning_v2.excess_reward import MatchedHoldReward
from moloch.learning_v2.interval_excess_reward import IntervalHoldReward


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
    if objective not in ['marked_wealth_return','matched_hold_excess','realized_pnl','interval_hold_lower_excess']:raise ValueError('UNKNOWN_TRAINING_REWARD_OBJECTIVE')
    hold_reward=None;interval_reward=None;interval_observation=None;excess_observation=None;previous_realized=0
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
    historical=protocol.get('historical_wallet')
    if historical:
        if protocol.get('initial_inventory_allocation_ppm',0) or objective not in ['interval_hold_lower_excess','realized_pnl']:
            raise ValueError('HISTORICAL_WALLET_REQUIRES_EXPLICIT_PARTIAL_WEALTH_OBJECTIVE')
        from moloch.learning_v2.historical_wallet import install_wallet
        installed=install_wallet(env,historical)
    behavior = fingerprint(model); hidden = model.initial_state(1)
    rows=[]; families=Counter(); missing=0; previous=initial; peak_committed=0.;tradable_frames=0;tradable_market_frames=0;observable_market_frames=0;begun=time.perf_counter();initial_portfolio=installed if historical else None
    initial_observed=None if historical else initial
    for step,at in enumerate(range(start-protocol['recurrent_burnin_ms'],end,protocol['decision_interval_ms'])):
        env.advance(at)
        if at==start and historical:
            initial_observed=env.marked_wealth()
            initial_portfolio['initial_marked_wealth_micros']=initial_observed
        if at==start and protocol.get('initial_inventory_allocation_ppm',0):
            initial_portfolio=seed_inventory(env,protocol['initial_inventory_allocation_ppm'],protocol['initial_inventory_seed'],
                                            selection=protocol.get('initial_inventory_selection','shuffle'))
            initial=initial_portfolio['initial_marked_wealth_micros'];previous=initial;env.initial_wealth=initial
        if at>=start and hold_reward is None:hold_reward=MatchedHoldReward(env.account(),initial)
        if at>=start and objective=='interval_hold_lower_excess' and interval_reward is None:
            from moloch.learning_v2.interval_episodes import IntervalEpisode
            if not isinstance(env,IntervalEpisode):raise ValueError('INTERVAL_EPISODE_REQUIRED')
            interval_reward=IntervalHoldReward(env.account(),env.sizing_capital_micros)
        obs=env.observe()
        if (model.contract_features,model.global_features)not in[(20,12),(CONTRACT_FEATURES,GLOBAL_FEATURES),(32,16),(40,16)]:
            raise ValueError('UNKNOWN_FEATURE_SEMANTICS')
        if model.contract_features in (32,40) and (obs['contracts'].shape[-1]!=model.contract_features or len(obs['global_state'])!=16):
            raise ValueError('OBSERVED_INTERVAL_FEATURES_REQUIRED')
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
        elif objective=='interval_hold_lower_excess':
            interval_observation=interval_reward.observe(env.account(),env.yes_intervals())if interval_reward else None
            reward=interval_observation.normalized_lower_increment if interval_observation else 0.
            valid=interval_observation is not None and interval_observation.available
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
    qualified=final is not None and initial_observed is not None and not unresolved
    terminal_excess=hold_reward.observe(final,env.marks())
    if objective=='interval_hold_lower_excess':
        interval_observation=interval_reward.observe(env.account(),env.yes_intervals())
        rows[-1]['reward']+=interval_observation.normalized_lower_increment
    elif objective=='matched_hold_excess':
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
           'interval_lower_excess_micros':interval_observation.lower_excess_micros if interval_observation else None,
           'interval_upper_excess_micros':interval_observation.upper_excess_micros if interval_observation else None,
           'qualified_relative_interval_accounting':bool(interval_observation and interval_observation.available and not unresolved),
           'terminal_excess_wealth_micros':terminal_excess.excess_wealth_micros,
           'start_ms':start,'decision_end_ms':end,'terminal_ms':terminal,'decisions':length//protocol['decision_interval_ms'],
           'recurrent_burnin_frames':protocol['recurrent_burnin_ms']//protocol['decision_interval_ms'],'split_role':split_role,
           'tradable_decision_frames':tradable_frames,'tradable_market_frames':tradable_market_frames,
           'observable_market_frames':observable_market_frames,
           'rollout_wall_seconds':time.perf_counter()-begun,'baseline':baseline,'qualified_accounting':qualified,
           'initial_reference_wealth_micros':initial_observed if historical else initial,'synthetic_initial_portfolio':initial_portfolio,
           'historical_wallet_used':bool(historical),'return_normalization_capital_micros':initial,
           'modeled_realized_return':env.realized/initial,'realized_pnl_includes_fees':True,
           'final_reference_wealth_micros':final,'reference_return':(final-(initial_observed if historical else initial))/(initial_observed if historical else initial) if qualified else None,
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
