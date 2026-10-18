"""Finite sequential-residency PBT study, never a production account client."""
import argparse
import gc
import json
from pathlib import Path
import random
import time
import torch
from .episodes import Tape, CONTRACT_FEATURES, GLOBAL_FEATURES
from .policy import RecurrentPolicy
from .population import (OPERATORS,OPERATOR_WEIGHTS,mutate,probe,distance,
                         change_hyperparameters,BehaviorArchive,founder_counts)
from .run_experiment import sha,write_json,load_mechanics,rollout
from .train_inner import PPOConfig,PPOTrainer,Trajectory,fingerprint
from .execution_profiles import load_profiles


def load_trainer(path,expected_sha,protocol_sha,device):
    if sha(path)!=expected_sha:raise ValueError('POPULATION_CHECKPOINT_CHANGED')
    b=torch.load(path,map_location=device,weights_only=True)
    if b['split_manifest_sha256']!=protocol_sha or b['production_authority'] is not False:
        raise ValueError('POPULATION_CHECKPOINT_ROLE_OR_AUTHORITY')
    model=RecurrentPolicy(b['contract_features'],b['global_features'],b['hidden_size']).to(device)
    model.load_state_dict(b['model']);trainer=PPOTrainer(model,PPOConfig(**b['config']),protocol_sha)
    trainer.optimizer.load_state_dict(b['optimizer']);trainer.policy_epoch=b['policy_epoch'];trainer.training_decisions=b['training_decisions']
    trainer.accepted_optimizer_updates=b.get('accepted_optimizer_updates',0)
    if fingerprint(model)!=b['parameter_sha256']:raise ValueError('POPULATION_PARAMETER_DIGEST')
    return trainer


def score(evaluations):
    if any(not e['qualified_accounting'] or e['reference_return'] is None or e.get('observable_market_frames',0)<=0 for e in evaluations):return -1e99
    return min(e['reference_return']for e in evaluations)


def main():
    p=argparse.ArgumentParser();p.add_argument('--dataset',type=Path,required=True);p.add_argument('--mechanics',type=Path,required=True)
    p.add_argument('--protocol',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--members',type=int,default=32);p.add_argument('--islands',type=int,default=4)
    p.add_argument('--cycles',type=int,default=2);p.add_argument('--seed',type=int,default=7001);p.add_argument('--device',default='cuda')
    p.add_argument('--execution-profiles',type=Path)
    p.add_argument('--founder-grace-blocks',type=int,default=2)
    p.add_argument('--replacement-mode',choices=['independent','local','multiscale'],default='multiscale');args=p.parse_args()
    if args.output.exists()or args.members%args.islands or args.members//args.islands<4 or args.cycles<1 or args.founder_grace_blocks<1:
        raise ValueError('FINITE_NEW_POPULATION_WITH_AT_LEAST_FOUR_PER_ISLAND_REQUIRED')
    args.output.mkdir(parents=True);(args.output/'checkpoints').mkdir();(args.output/'anchors').mkdir()
    torch.set_num_threads(4);rng=random.Random(args.seed);torch.manual_seed(args.seed)
    if args.device=='cuda':torch.cuda.reset_peak_memory_stats()
    begun=time.perf_counter();tape=Tape(args.dataset);mechanics=load_mechanics(args.mechanics)
    profiles=load_profiles(args.execution_profiles,mechanics)
    base=json.loads(args.protocol.read_text())
    if tape.manifest_sha256!=base['initial_dataset_manifest_sha256']:raise ValueError('POPULATION_DATASET_PROTOCOL')
    minimum=max(tape.times[0],base['run_start_ms'])+base['recurrent_burnin_ms']
    maximum=base['fit_end_ms']-base['episode_length_ms']-base['maximum_receipt_drain_ms']
    train_starts=[[random.Random(args.seed+c*10000+i).randint(minimum,maximum)for i in range(args.islands)]for c in range(args.cycles)]
    search_starts=base.get('search_evaluation_starts_ms',[base['search_start_ms']+60000,base['search_start_ms']+390000])
    source={q.name:sha(q)for q in sorted(Path(__file__).parent.glob('*.py'))if not q.name.startswith('test_')}
    plan={'schema_version':1,'classification':'conditional_development_population_not_live_qualification',
          'base_protocol':base,'base_protocol_sha256':sha(args.protocol),'mechanics_manifest_sha256':sha(args.mechanics),
          'dataset_manifest_sha256':tape.manifest_sha256,'members':args.members,'islands':args.islands,'cycles':args.cycles,
          'resident_gpu_learners':1,'seed':args.seed,'training_starts_by_cycle_island':train_starts,
          'search_starts_ms':search_starts,'search_episode_length_ms':150000,'fixed_terminal_drain_ms':120000,
          'bankroll_curriculum_by_island_cycle':[25000,100000,400000,1000000,2000000],
          'recorded_depth_or_mechanics_scaled_with_bankroll':False,'search_bankroll_micros':base['initial_cash_micros'],
          'replacement_operator_probabilities':dict(zip(OPERATORS,OPERATOR_WEIGHTS)),
          'replacement_mode':args.replacement_mode,
          'execution_profiles_sha256':sha(args.execution_profiles)if args.execution_profiles else None,
          'execution_profiles':json.loads(args.execution_profiles.read_text(encoding='utf-8'))if args.execution_profiles else None,
          'execution_profile_names':[name for name,_ in profiles],
          'training_execution_schedule':'cycle_index_mod_profile_count_common_to_all_islands',
          'selection_criterion':'minimum_reference_return_across_all_fixed_profiles_and_calendars',
          'weak_fraction_per_island':.25,'protected_elites_per_island':2,'maximum_global_founder_share':max(2,args.members//4)/args.members,
          'initial_founder_grace_training_blocks':args.founder_grace_blocks,
          'reset_branch_grace_training_blocks':2,'migration_probability':.20,'source_sha256':source,
          'search_is_optimization_not_locked_evidence':True,'locked_test_used':False,'production_authority':False,
          'matched_compute_ablation_complete':False,'projected_mutation_distance_available':False}
    write_json(args.output/'protocol.json',plan);protocol_sha=sha(args.output/'protocol.json')
    train_protocol={**base,'_sha256':protocol_sha};search_protocol={**train_protocol,'episode_length_ms':150000}
    if search_starts[-1]+150000+120000>min(tape.times[-1],base['run_start_ms']+base['run_duration_ms']):raise ValueError('INCOMPLETE_SEARCH_BLOCKS')
    archive=BehaviorArchive();members=[];history=[];genealogy=[];width=args.members//args.islands
    for i in range(args.members):
        identity=f'founder-{args.seed+i}'
        members.append({'id':identity,'founder':identity,'island':i//width,'seed':args.seed+i,'parent':None,
                        'checkpoint':None,'age':0,'grace':args.founder_grace_blocks,'operator':'fresh','lineage':[]})
    def provenance(member):
        return {'dataset_manifest_sha256':tape.manifest_sha256,'mechanics_manifest_sha256':sha(args.mechanics),
                'evaluation_protocol_sha256':protocol_sha,'action_adapter_sha256':source['episodes.py'],
                'lineage':{k:member[k]for k in ['id','founder','parent','operator','lineage','island']},
                'classification':'conditional_inspected_history_population_development_only','source_sha256':source}
    for cycle in range(args.cycles):
        summaries=[]
        for slot,m in enumerate(members):
            started=time.perf_counter();torch.manual_seed(m['seed']+cycle*100000)
            if m['checkpoint']:
                trainer=load_trainer(Path(m['checkpoint']),m['checkpoint_sha256'],protocol_sha,args.device)
            else:trainer=PPOTrainer(RecurrentPolicy(CONTRACT_FEATURES,GLOBAL_FEATURES).to(args.device),PPOConfig(epochs=2),protocol_sha)
            cash=[25000,100000,400000,1000000,2000000][(m['island']+cycle)%5]*1000000
            tp={**train_protocol,'initial_cash_micros':cash}
            fit_profile,fit_mechanics=profiles[cycle%len(profiles)]
            t,fit=rollout(tape,fit_mechanics,tp,trainer.model,train_starts[cycle][m['island']]);fit['execution_profile']=fit_profile
            updates=[]
            if fit['qualified_accounting'] and bool(t.learn.any()):updates=trainer.update(t)
            weights=fingerprint(trainer.model);evaluations=[]
            for profile_name,eval_mechanics in profiles:
                for j,start in enumerate(search_starts):
                    torch.manual_seed(args.seed+900000+j)
                    et,e=rollout(tape,eval_mechanics,search_protocol,trainer.model,start,split_role='search_development')
                    e.update(parameter_sha256=weights,execution_profile=profile_name);evaluations.append(e);del et
            if fingerprint(trainer.model)!=weights:raise ValueError('SEARCH_CHANGED_POLICY')
            checkpoint=args.output/'checkpoints'/f'c{cycle:03d}-s{slot:03d}-{m["id"]}.pt'
            checkpoint_sha=trainer.save(checkpoint,provenance(m))
            anchors=args.output/'anchors'/f'c{cycle:03d}-s{slot:03d}.pt'
            with anchors.open('xb')as f:torch.save(vars(t),f)
            m.update(checkpoint=str(checkpoint.resolve()),checkpoint_sha256=checkpoint_sha,anchors=str(anchors.resolve()),anchors_sha256=sha(anchors),age=m['age']+1)
            peaks=max(e['peak_cost_commitment_fraction']for e in evaluations);turnover=sum(e['gross_turnover_fraction']for e in evaluations)/len(evaluations)
            descriptor=(min(4,int(peaks*5)),min(4,int(turnover*10)),min(4,sum(e['fill_receipts']for e in evaluations)))
            admitted=archive.consider(m['id'],weights,evaluations,descriptor,str(checkpoint.resolve()))
            summary={'cycle':cycle,'slot':slot,'id':m['id'],'island':m['island'],'founder':m['founder'],
                     'fit':fit,'ppo_updates':updates,'search_evaluations':evaluations,'selection_score':None if score(evaluations)==-1e99 else score(evaluations),
                     'checkpoint_sha256':checkpoint_sha,'parameter_sha256':weights,'archive_admitted':admitted,'accepted_optimizer_updates':trainer.accepted_optimizer_updates,
                     'wall_seconds':time.perf_counter()-started,'classification':'development_selection_not_locked_test'}
            m['score']=score(evaluations);summaries.append(summary);history.append(summary)
            write_json(args.output/f'c{cycle:03d}-s{slot:03d}.json',summary)
            print(json.dumps({'phase':'member','cycle':cycle,'slot':slot,'returns':[e['reference_return']for e in evaluations],
                              'trained_decisions':trainer.training_decisions,'wall_seconds':summary['wall_seconds']}),flush=True)
            del trainer,t;gc.collect()
        write_json(args.output/f'archive-c{cycle:03d}.json',archive.json())
        if cycle+1==args.cycles:break
        if args.replacement_mode=='independent':continue
        selection_pool=[dict(m)for m in members]
        for island in range(args.islands):
            local=sorted([m for m in selection_pool if m['island']==island],key=lambda m:m['score'],reverse=True)
            protected={m['id']for m in local[:2]};weak=[m for m in reversed(local)if m['id']not in protected and m['age']>=m['grace']][:max(1,width//4)]
            for old in weak:
                operator='local'if args.replacement_mode=='local'else rng.choices(OPERATORS,weights=OPERATOR_WEIGHTS)[0]
                migrating=rng.random()<.20
                pool=sorted(selection_pool if migrating else local,key=lambda m:m['score'],reverse=True)[:max(2,args.members//4 if migrating else 2)]
                counts=founder_counts(members)
                pool=[m for m in pool if m['score']>-1e99 and counts[m['founder']]<max(2,args.members//4)and m['id']!=old['id']]
                parent=rng.choice(pool)if pool else None
                if parent is None:operator='fresh'
                seed=rng.randrange(1,2**31);identity=f'child-c{cycle+1}-{seed}';torch.manual_seed(seed)
                if operator=='fresh':
                    trainer=PPOTrainer(RecurrentPolicy(CONTRACT_FEATURES,GLOBAL_FEATURES).to(args.device),PPOConfig(epochs=2),protocol_sha)
                    child={**old,'id':identity,'founder':identity,'seed':seed,'parent':None,'lineage':[],'operator':operator,'age':0,'grace':2}
                    operation={'operator':operator,'independent_seed':seed,'optimizer_moments_reset':True}
                else:
                    trainer=load_trainer(Path(parent['checkpoint']),parent['checkpoint_sha256'],protocol_sha,args.device)
                    trainer.config=change_hyperparameters(trainer.config,rng)
                    if sha(parent['anchors'])!=parent['anchors_sha256']:raise ValueError('MUTATION_ANCHOR_CHANGED')
                    anchors=Trajectory(**torch.load(parent['anchors'],map_location=args.device,weights_only=True))
                    if operator in ['local','medium','large']:operation=mutate(trainer,anchors,operator,seed)
                    else:
                        before=probe(trainer.model,anchors)
                        for module in [*trainer.model.heads.values(),*trainer.model.conditioners.values(),trainer.model.fractions]:module.reset_parameters()
                        trainer.optimizer=torch.optim.Adam(trainer.model.parameters(),lr=trainer.config.learning_rate)
                        operation={'operator':operator,'seed':seed,**distance(before,probe(trainer.model,anchors)),'optimizer_moments_reset':True}
                    child={**old,'id':identity,'founder':parent['founder'],'parent':parent['id'],'seed':seed,
                           'lineage':parent['lineage']+[parent['id']],'operator':operator,'age':0,'grace':2 if operator=='head_reset'else 1}
                    del anchors
                cp=args.output/'checkpoints'/f'child-c{cycle+1}-{seed}.pt';cp_sha=trainer.save(cp,provenance(child))
                child.update(checkpoint=str(cp.resolve()),checkpoint_sha256=cp_sha)
                event={'at_cycle_boundary':cycle,'replaced':old['id'],'child':child['id'],'parent':child['parent'],
                       'island':island,'operation':operation,'fresh_trajectories_required':True,'protected_elites':sorted(protected)}
                genealogy.append(event);members[members.index(old)]=child
                print(json.dumps({'phase':'replacement',**event}),flush=True);del trainer;gc.collect()
        write_json(args.output/f'genealogy-c{cycle:03d}.json',genealogy)
    report={'protocol_sha256':protocol_sha,'source_sha256':source,'members':members,'history':history,'genealogy':genealogy,
            'archive':archive.json(),'total_wall_seconds':time.perf_counter()-begun,
            'peak_cuda_allocated_bytes':torch.cuda.max_memory_allocated()if args.device=='cuda'else None,
            'founder_counts':founder_counts(members),'locked_test_used':False,'production_changed':False,
            'competition_profitability_established':False,'matched_compute_ablation_complete':False}
    write_json(args.output/'report.json',report);write_json(args.output/'COMPLETE.json',{'report_sha256':sha(args.output/'report.json')})
    print(json.dumps({'phase':'complete','wall_seconds':report['total_wall_seconds'],'archive_size':len(archive.entries)}),flush=True)


if __name__=='__main__':main()
