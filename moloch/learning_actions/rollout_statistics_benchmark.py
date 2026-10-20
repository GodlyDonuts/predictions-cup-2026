"""Finite exact batched rollout comparison, without optimizer/account clients."""
import argparse,json,time
from pathlib import Path
from statistics import median
import torch
from .experiment import Experiment
from .batched import BatchStream,inputs
from .economics import GrowthLedger
from .stream import calibrate,tensors
from .training import load_checkpoint
from .episode_tape import financial_digest,tensor_digest
from .episode_training import release_worlds
from .selection import frozen
from moloch.learning_v2.train_inner import fingerprint


def compare(config,checkpoint,output,*,worlds=8,steps=64,prime_books=0):
    if config.get('mode')!='inspected_development'or config.get('test_finalist_once',False):
        raise ValueError('INSPECTED_TRAINING_ONLY_NO_INDEPENDENT_TEST')
    if any(type(n)is not int or n<1 for n in(worlds,steps)):raise ValueError('POSITIVE_FINITE_COMPUTE_SHAPE')
    if type(prime_books)is not int or prime_books< -1:raise ValueError('EXPLICIT_PRIMING_READ_COUNT')
    x=Experiment(config,output);device=config.get('device','cpu')
    def sync():
        if device.startswith('cuda'):torch.cuda.synchronize()
    try:
        m,b=load_checkpoint(checkpoint['path'],checkpoint['sha256'],'cpu')
        if b['dataset_manifest_sha256']!=x.dataset_sha:raise ValueError('EXACT_CHECKPOINT_DATASET_REQUIRED')
        original=b['parameter_sha256'];del b;m=m.to(device)
        m.acting_validation=config.get('acting_validation','consolidated_v1')
        a,z=x.train_bounds;length=min(config.get('development_episode_ms',900000),z-a)
        def make():
            envs=[x.world('train',a+min(120000+i*500,z-a-length),a+min(120000+i*500,z-a-length)+length)for i in range(worlds)]
            for e in envs:
                while prime_books and not any(k[0]=='book'for k in e.keys['SIG']):
                    if not e.clock._events or e.clock._events[0][0]>=e.end:raise ValueError('NO_BOOK_TARGET_BEFORE_TRAIN_EPISODE_END')
                    e.advance(e.clock._events[0][0])
                targets=[k for k in e.keys['SIG']if k[0]=='book']
                targets=targets if prime_books==-1 else targets[:prime_books]
                if prime_books>=0 and len(targets)!=prime_books:raise ValueError('EXACT_PRIMING_TARGET_COUNT_REQUIRED')
                for key in targets:
                    while True:
                        r=e.request_observation('SIG',key)
                        if r['status']=='IN_FLIGHT':e.advance(e.busy_until);break
                        if r['status']not in('BUSY','RATE_WAIT')or not e.now<r['ready_ms']<e.end:
                            raise ValueError('PAID_PRIMING_MUST_STAY_IN_TRAIN_EPISODE')
                        e.advance(r['ready_ms'])
                e._priming_count=len(targets)
            return envs
        warm=make();calibration=calibrate(warm[0],m);release_worlds(warm);del warm
        frozen(Path(output)/'COMMON_CALIBRATION.json',calibration)
        reference=None;measurements=[]
        for order in((False,True),(True,False)):
            for compact in order:
                envs=make();stream=BatchStream(envs,m,[GrowthLedger(e,development=True)for e in envs],
                    calibration=calibration,behavior_guard='tensor_versions_v1',rollout_only=compact)
                torch.manual_seed(x.seed+410000);trace=[];sync();begin=time.perf_counter()
                for _ in range(steps):
                    frame=stream.step()
                    if frame is None:break
                    trace.append({'actions':frame['actions'],'logprob':frame['old_logprob'],'value':frame['old_value']})
                prepared=inputs([e.observe()for e in envs],device)
                args={**prepared,'hidden':stream.hidden,'reset':torch.zeros(worlds,dtype=torch.bool,device=device)}
                with torch.no_grad():bootstrap=m.bootstrap_value(**args)if compact else m.step(**args,deterministic=True)['value']
                sync();elapsed=time.perf_counter()-begin
                values={k:torch.stack([r['actions'][k]for r in trace])for k in trace[0]['actions']}
                values.update({k:torch.stack([r[k]for r in trace])for k in('logprob','value')})
                values.update(hidden=stream.hidden,bootstrap=bootstrap)
                state={'traceAndBootstrap':tensor_digest(values),'financial':[financial_digest(e)for e in envs],
                    'observations':[tensor_digest(tensors(e.observe(),'cpu'))for e in envs],
                    'budgets':[repr(vars(e.budget))for e in envs],
                    'rng':bytes((torch.cuda.get_rng_state()if device.startswith('cuda')else torch.get_rng_state()).tolist()).hex(),
                    'counts':stream.decisions}
                if reference is None:reference=state
                if state!=reference:
                    frozen(Path(output)/'FAILED_COMPARISON.json',{'mode':compact,'differentFields':[k for k in state if state[k]!=reference[k]],
                        'independentTestUsed':False,'productionChanged':False})
                    raise ValueError('EXACT_ROLLOUT_STATISTICS_COMPARISON_FAILED')
                measurements.append({'compact':compact,'seconds':elapsed,'decisions':sum(stream.decisions),
                    'entities':[len(e.observe()['entities'])for e in envs],'primingReads':[e._priming_count for e in envs]})
                release_worlds(envs);del envs,stream,trace,frame,prepared,args,values,bootstrap
        if fingerprint(m)!=original:raise ValueError('BENCHMARK_CHANGED_POLICY_PARAMETERS')
        times={label:median(r['seconds']for r in measurements if r['compact']==compact)
            for label,compact in(('full',False),('compact',True))}
        report={'classification':'finite_exact_batched_rollout_comparison_not_ppo_or_roi','device':device,
            'parameters':sum(p.numel()for p in m.parameters()),'worlds':worlds,'stepsPerWorld':steps,
            'measurements':measurements,'medianSeconds':times,'speedup':times['full']/times['compact'],
            'exactActionsJointDensityValuesHiddenBootstrapRngFinanceKnowledgeClocksBudgets':True,
            'runtimeSourceSha256':x.frozen['source_sha256'],'datasetManifestSha256':x.dataset_sha,
            'checkpointSha256':checkpoint['sha256'],'calibration':calibration,
            'optimizerRun':False,'fullEpisodesCompleted':False,'accountRequests':0,
            'independentTestUsed':False,'productionChanged':False,'roiEstablished':False}
        frozen(Path(output)/'RESULT.json',report);return report
    finally:x.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--config',type=Path,required=True)
    p.add_argument('--checkpoint',type=Path,required=True);p.add_argument('--checkpoint-sha256',required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--worlds',type=int,default=8)
    p.add_argument('--steps',type=int,default=64);p.add_argument('--prime-books',type=int,default=0)
    a=p.parse_args();r=compare(json.loads(a.config.read_text()),{'path':str(a.checkpoint),'sha256':a.checkpoint_sha256},
        a.output,worlds=a.worlds,steps=a.steps,prime_books=a.prime_books)
    print(json.dumps({k:r[k]for k in('device','medianSeconds','speedup')}),flush=True)
