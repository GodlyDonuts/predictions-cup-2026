"""Finite exact serial-inference comparison on inspected development history."""
import argparse,json,time
from pathlib import Path
from statistics import median
import torch
from .acting_validation import STRICT,CONSOLIDATED
from .experiment import Experiment
from .training import load_checkpoint
from .stream import Stream,tensors,calibrate
from .episode_tape import financial_digest,tensor_digest
from .selection import frozen
from moloch.learning_v2.train_inner import fingerprint


def compare(config,checkpoint,output,*,steps=64):
    if config.get('mode')!='inspected_development'or config.get('test_finalist_once',False):
        raise ValueError('INSPECTED_DEVELOPMENT_ONLY_NO_INDEPENDENT_TEST')
    if type(steps)is not int or steps<1:raise ValueError('POSITIVE_FINITE_BENCHMARK_CHUNK')
    x=Experiment(config,output);device=config.get('device','cpu')
    try:
        m,b=load_checkpoint(checkpoint['path'],checkpoint['sha256'],'cpu')
        if b['dataset_manifest_sha256']!=x.dataset_sha:raise ValueError('EXACT_CHECKPOINT_DATASET_REQUIRED')
        original=b['parameter_sha256'];del b;m=m.to(device)
        a,z=x.evolution_bounds;middle=a+(z-a)//2
        descriptors=[(lo,hi,s)for lo,hi in((a,middle),(middle,z))for s in(False,True)]
        def world(descriptor):
            lo,hi,stress=descriptor;e=x.world('evolution',lo,hi,stress=stress);opened=e.now
            while not any(k[0]=='book'for k in e.keys['SIG']):
                if not e.clock._events or e.clock._events[0][0]>=e.end:raise ValueError('NO_SIG_BOOK_SOURCE_BEFORE_ROLE_END')
                e.advance(e.clock._events[0][0])
            targets=[k for k in e.keys['SIG']if k[0]=='book']
            for key in targets:
                while True:
                    r=e.request_observation('SIG',key)
                    if r['status']=='IN_FLIGHT':e.advance(e.busy_until);break
                    if r['status']not in('BUSY','RATE_WAIT')or not e.now<r['ready_ms']<e.end:
                        raise ValueError('PAID_PRIMING_MUST_STAY_IN_ROLE')
                    e.advance(r['ready_ms'])
            e._primed_reads=len(targets);e._warmup_ms=e.now-opened;return e
        initial=world(descriptors[0]);m.acting_validation=STRICT
        calibration=calibrate(initial,m);frozen(Path(output)/'COMMON_CALIBRATION.json',calibration)
        inputs=tensors(initial.observe(),device);hidden=m.initial_state(1);reset=torch.ones(1,device=device,dtype=torch.bool)
        profiles={}
        for mode in(STRICT,CONSOLIDATED):
            m.acting_validation=mode
            with torch.no_grad():
                with torch.profiler.profile(activities=[torch.profiler.ProfilerActivity.CPU])as p:
                    m.step(**inputs,hidden=hidden,reset=reset)
            profiles[mode]={k.key:k.count for k in p.key_averages()if any(s in k.key for s in
                ('is_nonzero','_local_scalar_dense','cudaStreamSynchronize','cudaDeviceSynchronize'))}
        from .episode_training import release_worlds
        release_worlds([initial]);del initial,inputs,hidden
        measurements=[];reference=None
        cpu_rng=torch.get_rng_state().clone();gpu_rng=torch.cuda.get_rng_state()if device.startswith('cuda')else None
        try:
            for modes in((STRICT,CONSOLIDATED),(CONSOLIDATED,STRICT)):
                for mode in modes:
                    envs=[world(d)for d in descriptors];m.acting_validation=mode
                    streams=[Stream(e,m,calibration=calibration)for e in envs]
                    if device.startswith('cuda'):torch.cuda.synchronize()
                    begin=time.perf_counter();trace=[];rng=[]
                    for i,s in enumerate(streams):
                        torch.manual_seed(x.seed+90000+i);rows=[]
                        for _ in range(steps):
                            if s.env.now>=s.env.end:break
                            rows.append(s.step())
                        trace.append(rows)
                        rng.append((torch.cuda.get_rng_state()if device.startswith('cuda')else torch.get_rng_state()).cpu().numpy().tobytes())
                    if device.startswith('cuda'):torch.cuda.synchronize()
                    seconds=time.perf_counter()-begin
                    state={'trace':trace,'rng':rng,'finance':[financial_digest(e)for e in envs],
                        'observation':[tensor_digest(tensors(e.observe(),'cpu'))for e in envs],
                        'hidden':[s.hidden.detach().cpu().numpy().tobytes()for s in streams],
                        'budgets':[repr(vars(e.budget))for e in envs]}
                    if reference is None:reference=state
                    if state!=reference:
                        frozen(Path(output)/'FAILED_COMPARISON.json',{'mode':mode,
                            'differentFields':[k for k in state if state[k]!=reference[k]],
                            'independentTestUsed':False,'productionChanged':False})
                        raise ValueError('EXACT_SERIAL_VALIDATION_COMPARISON_FAILED')
                    measurements.append({'mode':mode,'seconds':seconds,'decisions':sum(s.decisions for s in streams),
                        'primingReads':[e._primed_reads for e in envs],'warmupVirtualMs':[e._warmup_ms for e in envs],
                        'entities':[len(e.observe()['entities'])for e in envs]})
                    release_worlds(envs);del envs,streams,state
        finally:
            torch.set_rng_state(cpu_rng)
            if gpu_rng is not None:torch.cuda.set_rng_state(gpu_rng)
        if fingerprint(m)!=original:raise ValueError('BENCHMARK_CHANGED_POLICY_PARAMETERS')
        medians={mode:median(r['seconds']for r in measurements if r['mode']==mode)for mode in(STRICT,CONSOLIDATED)}
        result={'classification':'finite_exact_serial_inference_comparison_not_roi','device':device,'torchVersion':torch.__version__,
            'parameters':sum(p.numel()for p in m.parameters()),'architecture':m.architecture,'checkpointSha256':checkpoint['sha256'],
            'datasetManifestSha256':x.dataset_sha,'runtimeSourceSha256':x.frozen['source_sha256'],
            'measurements':measurements,'medianSeconds':medians,'speedup':medians[STRICT]/medians[CONSOLIDATED],
            'forwardSynchronizationCounts':profiles,'commonVirtualDecisionCalibration':calibration,
            'exactActionsRngFinanceKnowledgeHiddenClocksAndBudgets':True,'policyUnchanged':True,
            'accountRequests':0,'independentTestUsed':False,'productionChanged':False,'roiEstablished':False,
            'fullEpisodeEvaluationCompleted':False}
        frozen(Path(output)/'RESULT.json',result);return result
    finally:x.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--config',type=Path,required=True)
    p.add_argument('--checkpoint',type=Path,required=True);p.add_argument('--checkpoint-sha256',required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--steps',type=int,default=64)
    a=p.parse_args();r=compare(json.loads(a.config.read_text()),{'path':str(a.checkpoint),'sha256':a.checkpoint_sha256},a.output,steps=a.steps)
    print(json.dumps({k:r[k]for k in('device','medianSeconds','speedup','forwardSynchronizationCounts')}),flush=True)
