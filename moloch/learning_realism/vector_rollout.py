"""Batch policy inference with GPU-resident recurrent memory, no actor threads.

Each world retains original event order, information delays, wallet and limiter.
Only observations/actions cross the device boundary. Optional bounded complete
trajectories can be passed directly to the existing PPO trainer. This preserves
the existing interval-HOLD development objective; it is not the new daily-score
experiment or a claim that training data/held-out coverage is ready.
"""
from collections import Counter
import time
import numpy as np
import torch
from moloch.learning_v2.interval_excess_reward import IntervalHoldReward
from moloch.learning_v2.packed_io import transfer
from moloch.learning_v2.train_inner import Trajectory, fingerprint


def _inputs(observations, device, reset):
    cpu = {k: torch.from_numpy(np.stack([o[k] for o in observations]))
           for k in ('contracts', 'global_state', 'present')}
    cpu['masks'] = {k: torch.from_numpy(np.stack([o['masks'][k] for o in observations]))
                    for k in observations[0]['masks']}
    cpu['reset'] = torch.full((len(observations),), reset, dtype=torch.bool)
    return transfer(cpu, device)


def collect(tape, mechanics, model, starts, *, steps, interval_ms=1000, burnin_steps=10,
            drain_ms=60000, initial_cash_micros=100000000000,
            target_wealth_micros=1100000000000, deterministic=False,
            inference_mode='batched', capture_trajectory=True, split_manifest_sha256=None):
    """Complete recurrent episodes, bounded to avoid unbounded GPU trajectories.

    Scalar mode is an equivalent reference path using the same batch-shaped
    hidden storage. Deterministic mode supports differential verification;
    stochastic batch draws need not match a scalar RNG stream bit-for-bit.
    """
    if not starts or len(starts) > 64 or not 1 <= steps <= 1024 or interval_ms <= 0 or not 0 <= burnin_steps < steps or drain_ms <= 0:
        raise ValueError('FINITE_COMPLETE_VECTOR_EPISODES_REQUIRED')
    if inference_mode not in ('scalar', 'batched'): raise ValueError('EXPLICIT_INFERENCE_MODE')
    if capture_trajectory and (not isinstance(split_manifest_sha256, str) or len(split_manifest_sha256) != 64):
        raise ValueError('PINNED_TRAJECTORY_PROTOCOL_REQUIRED')
    if tape.role == 'test' and capture_trajectory:
        raise ValueError('INDEPENDENT_TEST_CANNOT_PRODUCE_TRAINING_TRAJECTORY')
    device = next(model.parameters()).device
    behavior = fingerprint(model) if capture_trajectory else None
    envs = [tape.episode(mechanics, start, start+(steps-burnin_steps)*interval_ms+drain_ms,
        initial_cash_micros=initial_cash_micros, target_wealth_micros=target_wealth_micros,
        burnin_ms=burnin_steps*interval_ms, advance_to_start=False) for start in starts]
    hidden = model.initial_state(len(envs)).to(device)
    rewards = [None]*len(envs); families = [Counter() for _ in envs]
    rows = []; hashes = []; traces=[]; env_seconds = 0.; pack_seconds = 0.; actor_seconds = 0.; action_seconds = 0.
    begun = time.perf_counter()
    for step in range(steps):
        at = time.perf_counter(); observations = []
        for i, env in enumerate(envs):
            env.advance(starts[i]+(step-burnin_steps)*interval_ms)
            if step == burnin_steps: rewards[i] = IntervalHoldReward(env.account(), env.sizing_capital_micros)
            observations.append(env.observe())
        env_seconds += time.perf_counter()-at
        at = time.perf_counter(); inputs = _inputs(observations, device, step == 0)
        pack_seconds += time.perf_counter()-at
        at = time.perf_counter()
        with torch.no_grad():
            if inference_mode == 'batched':
                out = model.step(**inputs, hidden=hidden, deterministic=deterministic)
            else:
                outputs = [model.step(**{k: ({n:v[i:i+1] for n,v in value.items()} if isinstance(value,dict) else value[i:i+1])
                    for k,value in inputs.items()}, hidden=hidden[i:i+1], deterministic=deterministic) for i in range(len(envs))]
                out = {k: ({n: torch.cat([o[k][n] for o in outputs]) for n in value}
                           if isinstance(value,dict) else torch.cat([o[k] for o in outputs])) for k,value in outputs[0].items()}
            hidden = out['hidden']
            actions = transfer(out['actions'], 'cpu')
        # The action copy synchronizes CUDA; timing includes actual completion.
        actor_seconds += time.perf_counter()-at
        at = time.perf_counter(); monetary = []; learn = []
        for i, env in enumerate(envs):
            action = {k:value[i].item() for k,value in actions.items()}
            if step < burnin_steps: action['family'] = 0
            else: families[i][str(action['family'])] += 1
            env.act(action)
            env.advance(starts[i]+(step-burnin_steps+1)*interval_ms)
            r = rewards[i].observe(env.account(),env.yes_intervals()) if rewards[i] is not None else None
            monetary.append(r.normalized_lower_increment if r is not None else 0.)
            learn.append(bool(r is not None and r.available))
        action_seconds += time.perf_counter()-at
        if capture_trajectory:
            rows.append({**inputs, 'actions':out['actions'], 'old_logprob':out['logprob'], 'old_values':out['value'],
                'rewards':torch.tensor(monetary,device=device), 'learn':torch.tensor(learn,device=device),
                'done':torch.full((len(envs),),step==steps-1,dtype=torch.bool,device=device),
                'elapsed_ms':torch.full((len(envs),),interval_ms,dtype=torch.int64,device=device),
                'target_success_labels':torch.full((len(envs),),float('nan'),device=device)})
        if deterministic:
            traces.append({k:v.tolist()for k,v in actions.items()})
            import hashlib
            h=hashlib.sha256()
            for key,value in actions.items(): h.update(key.encode());h.update(value.numpy().tobytes())
            hashes.append(h.hexdigest())
    drain_rewards=[]; unresolved=[];qualified=[]
    for i,env in enumerate(envs):
        end = env.start+(steps-burnin_steps)*interval_ms
        env.advance(end)
        # Match the original finite-episode rule: unsent drafts never turn into
        # new entries during drain, and resting cancels still cost writes.
        for qid in [q for q,o in env.orders.items() if o.phase=='preflight']:
            env.orders.pop(qid)
            env.execution_events.append({'order':qid,'at':env.now,'kind':'not_sent','reason':'FINITE_DECISION_WINDOW_ENDED'})
        for at in range(end,end+drain_ms+1,interval_ms):
            env.advance(at)
            if env.budget.active is None:
                for order in list(env.orders.values()):
                    if env.cancel(order.id): break
        env.advance(end+drain_ms)
        observation=rewards[i].observe(env.account(),env.yes_intervals())
        drain_rewards.append(observation.normalized_lower_increment)
        unresolved.append(bool(env.orders or env.budget.unknown or
                               (env.budget.active is not None and env.budget.active[1])))
        qualified.append(bool(observation.available and not unresolved[-1]))
    elapsed=time.perf_counter()-begun
    trajectory=None
    if capture_trajectory:
        rows[-1]['rewards']+=torch.tensor(drain_rewards,device=device)
        rows[-1]['elapsed_ms']+=drain_ms
        trajectory=Trajectory(**{k: ({n:torch.stack([row[k][n] for row in rows])for n in value}if isinstance(value,dict)
                                    else torch.stack([row[k]for row in rows]))for k,value in rows[0].items()},
            behavior_sha256=behavior,split_manifest_sha256=split_manifest_sha256,
            split_role={'train':'fit_training','evolution':'search_development'}[tape.role])
        if fingerprint(model)!=behavior: raise ValueError('COLLECTOR_CHANGED_POLICY')
    report={'mode':inference_mode,'device':str(device),'worlds':len(envs),'steps_per_world':steps,
        'wall_seconds':elapsed,'simulated_decision_seconds':steps*interval_ms/1000*len(envs),
        'aggregate_replay_speed':steps*interval_ms/1000*len(envs)/elapsed,
        'env_observe_seconds':env_seconds,'pack_seconds':pack_seconds,'actor_seconds':actor_seconds,
        'action_and_advance_seconds':action_seconds,'deterministic_action_hashes':hashes,
        'deterministic_action_trace':traces,
        'worlds_summary':[{'cash':e.cash,'realized':e.realized,'lots':{str(k):v for k,v in e.lots.items()},
            'fills':e.fill_events,'reads':len([r for r in e.budget.request_ids if r.startswith('feed:SIG:')or':read:'in r]),
            'writes':len([r for r in e.budget.request_ids if r.endswith(':submit')or r.endswith(':cancel')]),
            'feed_requests':dict(e.feed_request_counts),'families':dict(families[i]),
            'qualified_relative_interval_accounting':qualified[i]}for i,e in enumerate(envs)],
        'source_timing_or_budgets_changed':False}
    report['unresolved_worlds']=sum(unresolved)
    report['qualified_worlds']=sum(qualified)
    return trajectory,report
