"""Finite CUDA-batched training with immutable development/evaluation roles."""
import argparse
from dataclasses import replace
import json
from pathlib import Path
import random
import shutil
import subprocess
import threading
import time
import torch
from .batched_rollout import rollout_batch
from .episodes import Tape
from .execution_profiles import load_profiles
from .fused_policy import FusedRecurrentPolicy
from .run_experiment import load_mechanics,sha,write_json
from .train_inner import PPOTrainer,PPOConfig,fingerprint


def telemetry(path,stop):
    with path.open('x',encoding='utf-8')as out:
        while not stop.is_set():
            try:
                raw=subprocess.check_output(['nvidia-smi','--query-gpu=utilization.gpu,memory.used,power.draw','--format=csv,noheader,nounits'],text=True,timeout=5)
                out.write(json.dumps({'at_ms':time.time_ns()//1000000,'gpu_csv':raw.strip()})+'\n');out.flush()
            except (OSError,subprocess.SubprocessError):pass
            stop.wait(1)


def main():
    p=argparse.ArgumentParser()
    for flag in ['dataset','mechanics','protocol','execution-profiles','output']:p.add_argument('--'+flag,type=Path,required=True)
    p.add_argument('--batch-worlds',type=int,default=16);p.add_argument('--blocks',type=int,default=16)
    p.add_argument('--hidden-size',type=int,default=1024);p.add_argument('--epochs',type=int,default=4)
    p.add_argument('--evaluate-every',type=int,default=4);p.add_argument('--seed',type=int,default=16001)
    p.add_argument('--cpu-processes',action='store_true');p.add_argument('--cpu-actors',type=int,default=4);args=p.parse_args()
    if min(args.batch_worlds,args.blocks,args.hidden_size,args.evaluate_every)<=0:raise ValueError('FINITE_POSITIVE_TRAINING_BOUNDS_REQUIRED')
    args.output.mkdir(parents=True,exist_ok=False);torch.set_num_threads(4);torch.manual_seed(args.seed)
    if not torch.cuda.is_available():raise ValueError('CUDA_REQUIRED')
    torch.cuda.reset_peak_memory_stats();begun=time.perf_counter();tape=Tape(args.dataset)
    base=json.loads(args.protocol.read_text());mechanics=load_mechanics(args.mechanics);profiles=load_profiles(args.execution_profiles,mechanics)
    if tape.manifest_sha256!=base['initial_dataset_manifest_sha256']or base['history_roles']!=['fit_training','search_development']:raise ValueError('FIXED_DEVELOPMENT_DATASET_ROLE_REQUIRED')
    minimum=max(tape.times[0],base['run_start_ms'])+base['recurrent_burnin_ms'];maximum=base['fit_end_ms']-base['episode_length_ms']-base['maximum_receipt_drain_ms']
    if maximum<minimum:raise ValueError('NO_COMPLETE_FIT_WINDOW')
    rng=random.Random(args.seed);starts=[[rng.randint(minimum,maximum)for _ in range(args.batch_worlds)]for _ in range(args.blocks)]
    source={x.name:sha(x)for x in Path(__file__).parent.glob('*.py')if not x.name.startswith('test_')}
    frozen=args.output/'source/moloch/learning_v2';frozen.mkdir(parents=True)
    for name in source:shutil.copyfile(Path(__file__).parent/name,frozen/name)
    model=FusedRecurrentPolicy(hidden_size=args.hidden_size).cuda()
    plan={'classification':'batched_development_training_not_locked_generalization_or_live_approval',
          'batch_worlds':args.batch_worlds,'blocks':args.blocks,'epochs':args.epochs,'hidden_size':args.hidden_size,
          'parameters':sum(p.numel()for p in model.parameters()),'architecture':model.architecture,
          'seed':args.seed,'cpu_actor_backend':'processes'if args.cpu_processes else'threads','cpu_actor_processes':min(args.batch_worlds,args.cpu_actors)if args.cpu_processes else 0,
          'fit_starts_ms':starts,'base_protocol':base,'base_protocol_sha256':sha(args.protocol),
          'dataset_manifest_sha256':tape.manifest_sha256,'mechanics_sha256':sha(args.mechanics),
          'execution_profiles_sha256':sha(args.execution_profiles),'source_sha256':source,
          'data_previously_inspected':True,'locked_data_used':False,'production_authority':False,
          'optimizer_epochs_are_bounded_not_a_perfect_model_claim':True}
    write_json(args.output/'plan.json',plan);split=sha(args.output/'plan.json');protocol={**base,'_sha256':split}
    trainer=PPOTrainer(model,PPOConfig(epochs=args.epochs),split);history=[];evaluations=[]
    stop=threading.Event();monitor=threading.Thread(target=telemetry,args=(args.output/'gpu-telemetry.jsonl',stop),daemon=True);monitor.start()
    pool=None
    try:
        if args.cpu_processes:
            from .process_rollout import ProcessRollouts
            pool=ProcessRollouts(args.dataset,tape.manifest_sha256,args.batch_worlds,args.cpu_actors)
        for block,world_starts in enumerate(starts):
            if any(sha(Path(__file__).parent/name)!=h for name,h in source.items()):raise ValueError('TRAINING_SOURCE_CHANGED')
            torch.manual_seed(args.seed+block*10000)
            label,m=profiles[block%len(profiles)]
            trajectory,results,perf=(pool.rollout(m,protocol,model,world_starts)if pool else rollout_batch(tape,m,protocol,model,world_starts))
            qualified=torch.tensor([e['qualified_accounting']for e in results],device='cuda')
            trajectory=replace(trajectory,learn=trajectory.learn&qualified.unsqueeze(0))
            at=time.perf_counter();updates=trainer.update(trajectory)if bool(trajectory.learn.any())else []
            row={'block':block,'execution_profile':label,'rollout':perf,'update_wall_seconds':time.perf_counter()-at,
                 'fit_results':results,'updates':updates,'accepted_optimizer_updates':trainer.accepted_optimizer_updates,
                 'peak_cuda_allocated_bytes':torch.cuda.max_memory_allocated(),'peak_cuda_reserved_bytes':torch.cuda.max_memory_reserved()}
            write_json(args.output/f'block-{block:04d}.json',row);history.append(row);del trajectory
            provenance={'dataset_manifest_sha256':tape.manifest_sha256,'mechanics_manifest_sha256':sha(args.mechanics),
                        'evaluation_protocol_sha256':split,'action_adapter_sha256':source['episodes.py'],
                        'lineage':{'id':f'batched-{args.seed}','block':block,'operator':'fresh_on_policy_fused_ppo'},
                        'classification':'development_only','source_sha256':source}
            checkpoint=args.output/f'checkpoint-{block:04d}.pt';checkpoint_sha=trainer.save(checkpoint,provenance)
            row['checkpoint_sha256']=checkpoint_sha
            print(json.dumps({'phase':'block','block':block,'rollout_wall_seconds':perf['wall_seconds'],'update_wall_seconds':row['update_wall_seconds'],
                              'qualified_worlds':int(qualified.sum()),'returns':[e['reference_return']for e in results],
                              'peak_cuda_reserved_bytes':row['peak_cuda_reserved_bytes']}),flush=True)
            if (block+1)%args.evaluate_every==0 or block+1==args.blocks:
                before=fingerprint(model);ep={**protocol,'episode_length_ms':150000};out=[]
                for name,mechanics in profiles:
                    torch.manual_seed(args.seed+900000)
                    t,r,profile=rollout_batch(tape,mechanics,ep,model,base['search_evaluation_starts_ms'],split_role='search_development');del t
                    out.append({'profile':name,'results':r,'performance':profile})
                if fingerprint(model)!=before:raise ValueError('EVALUATION_CHANGED_MODEL')
                e={'block':block,'parameter_sha256':before,'checkpoint_sha256':checkpoint_sha,'evaluations':out,'selection_data_not_locked_test':True}
                write_json(args.output/f'evaluation-{block:04d}.json',e);evaluations.append(e)
        report={'plan_sha256':split,'history':history,'evaluations':evaluations,'total_wall_seconds':time.perf_counter()-begun,
                'training_decisions':trainer.training_decisions,'accepted_optimizer_updates':trainer.accepted_optimizer_updates,
                'competition_profitability_established':False,'production_changed':False}
        write_json(args.output/'report.json',report);write_json(args.output/'COMPLETE.json',{'report_sha256':sha(args.output/'report.json')})
    finally:
        if pool:pool.close()
        stop.set();monitor.join(timeout=6)


if __name__=='__main__':main()
