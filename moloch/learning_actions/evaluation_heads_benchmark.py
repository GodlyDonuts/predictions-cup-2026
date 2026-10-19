"""Finite paired selection-compute comparison on already inspected history."""
import argparse,copy,json,time
from dataclasses import asdict
from pathlib import Path
import torch
from .experiment import Experiment
from .evaluation_batch import EvaluationBatch
from .stream import Stream,calibrate
from .economics import GrowthLedger
from .training import load_checkpoint
from .episode_tape import financial_digest
from .population_handoff import sha_file
from .selection import frozen


def compare(config,checkpoint,output,*,steps=64,prime_books=0,allow_sized_request_differences=False):
    if config.get('mode')!='inspected_development'or config.get('test_finalist_once',False):
        raise ValueError('INSPECTED_DEVELOPMENT_ONLY_NO_INDEPENDENT_TEST')
    if type(steps)is not int or steps<1:raise ValueError('POSITIVE_FINITE_BENCHMARK_CHUNK')
    if type(prime_books)is not int or prime_books< -1:raise ValueError('EXPLICIT_PRIMING_READS_OR_MINUS_ONE_FOR_ALL')
    if type(allow_sized_request_differences)is not bool:raise ValueError('EXPLICIT_NUMERICAL_COMPARISON_PROFILE')
    x=Experiment(config,output);device=config.get('device','cpu')
    try:
        # Avoid copying an unused historical optimizer onto the GPU.
        model,bundle=load_checkpoint(checkpoint['path'],checkpoint['sha256'],'cpu')
        if bundle['dataset_manifest_sha256']!=x.dataset_sha:raise ValueError('EXACT_BENCHMARK_DATASET_REQUIRED')
        original_parameters=bundle['parameter_sha256'];del bundle;model=model.to(device)
        a,b=x.evolution_bounds;middle=a+(b-a)//2
        descriptors=[(lo,hi,stress)for lo,hi in((a,middle),(middle,b))for stress in(False,True)]
        def worlds():
            envs=[x.world('evolution',lo,hi,stress=stress)for lo,hi,stress in descriptors]
            for e in envs:
                opened=e.now
                # The first selection case can open before its first SIG book
                # source event. Benchmark setup must wait on the original tape,
                # not invent a target, quote or earlier arrival timestamp.
                while prime_books and not any(k[0]=='book'for k in e.keys['SIG']):
                    if not e.clock._events or e.clock._events[0][0]>=e.end:
                        raise ValueError('NO_SIG_BOOK_TARGET_BEFORE_ROLE_END')
                    e.advance(e.clock._events[0][0])
                e._benchmark_source_warmup_ms=e.now-opened
                available=[k for k in e.keys['SIG']if k[0]=='book']
                targets=available if prime_books==-1 else available[:prime_books]
                if (prime_books==-1 and not targets)or(prime_books>=0 and len(targets)!=prime_books):
                    raise ValueError('EXACT_PRIMING_SOURCE_TARGETS_REQUIRED')
                e._benchmark_priming_read_count=len(targets)
                for key in targets:
                    while True:
                        r=e.request_observation('SIG',key)
                        if r['status']=='IN_FLIGHT':e.advance(e.busy_until);break
                        if r['status']not in('BUSY','RATE_WAIT'):raise ValueError('PRIMING_READ_REJECTED')
                        if not e.now<r['ready_ms']<e.end:raise ValueError('PRIMING_MUST_STAY_IN_DEVELOPMENT_ROLE')
                        e.advance(r['ready_ms'])
            return envs
        first=worlds();calibration=calibrate(first[0],model)
        from .episode_training import release_worlds
        release_worlds(first);del first
        seeds=[x.seed+90000+i for i in range(len(descriptors))]
        def sync():
            if device.startswith('cuda'):torch.cuda.synchronize()
        modes=('serial','batch_encoding','batch_heads');measurements=[];reference=None;reference_hidden=None;reference_traces=None;max_fraction_error=0.
        reference_full=None;reference_requests=None;request_differences=[]
        for order in(modes,tuple(reversed(modes))):
            for mode in order:
                envs=worlds();ledgers=[GrowthLedger(e,development=True,historical_last_mark=True)for e in envs]
                streams=[];batch=None
                if mode=='serial':streams=[Stream(e,model,calibration=calibration)for e in envs]
                else:batch=EvaluationBatch(envs,model,seeds,calibration=calibration,batch_action_heads=mode=='batch_heads')
                saved=(torch.cuda.get_rng_state()if device.startswith('cuda')else torch.get_rng_state())
                saved_cpu=torch.get_rng_state().clone()
                sync();start=time.perf_counter();counts=[];rngs=[];traces=[[]for _ in envs]
                try:
                    if mode=='serial':
                        for case,(seed,stream)in enumerate(zip(seeds,streams)):
                            torch.manual_seed(seed)
                            for _ in range(steps):
                                if stream.env.now>=stream.env.end:break
                                traces[case].append(stream.step())
                            counts.append(stream.decisions)
                            rngs.append((torch.cuda.get_rng_state()if device.startswith('cuda')else torch.get_rng_state()).clone())
                    else:
                        for _ in range(steps):
                            if not batch.step():break
                            for row in batch.last_rows:traces[row['case']].append(row)
                        counts=batch.decisions;rngs=batch.rng
                    sync();seconds=time.perf_counter()-start
                finally:
                    torch.set_rng_state(saved_cpu)
                    torch.cuda.set_rng_state(saved)if device.startswith('cuda')else torch.set_rng_state(saved)
                hidden=(torch.cat([s.hidden for s in streams])if mode=='serial'else batch.hidden).detach().cpu()
                if reference_hidden is None:reference_hidden=hidden
                torch.testing.assert_close(hidden,reference_hidden,atol=2e-5,rtol=2e-4)
                if reference_traces is None:reference_traces=traces
                for expected,actual in zip(reference_traces,traces):
                    if len(expected)!=len(actual):raise ValueError('EXACT_CASE_TRACE_LENGTH_REQUIRED')
                    for left,right in zip(expected,actual):
                        for key in('at_ms','next_ms','status','terminal'):
                            if left[key]!=right[key]:raise ValueError('EXACT_CASE_ACTION_CLOCK_AND_STATUS_REQUIRED')
                        for key,value in left['action'].items():
                            if key.endswith('fraction'):
                                error=abs(value-right['action'][key]);max_fraction_error=max(error,max_fraction_error)
                                if error>1e-5:raise ValueError('RAW_ACTION_LATENT_TOLERANCE_EXCEEDED')
                            elif value!=right['action'][key]:raise ValueError('EXACT_DISCRETE_ACTION_REQUIRED')
                full=[financial_digest(e)for e in envs];requests=[{q:o.spec.quantity for q,o in e.api_orders.items()}for e in envs]
                def sized_finance(e):
                    # Comparison view only. Actual orders/ACKs/local memory are
                    # never rewritten or rounded. All other finance is exact.
                    orders={q:asdict(o)for q,o in e.api_orders.items()}
                    for o in orders.values():o['spec']['quantity']=o['remaining']
                    return copy.deepcopy((e.now,e.cash,e.lots,e.realized,e.fees,orders,e.api_id,e.api_fills,e.finance_revision,
                        [(k,b.book,b.invalidated,sorted(b.depleted.items()))for k,b in sorted(e.books.items())]))
                state={'finance':[sized_finance(e)for e in envs],'clocks':[e.now for e in envs],
                       'budgets':[copy.deepcopy(vars(e.budget))for e in envs],
                       'counts':counts,'rng':[bytes(r.tolist()).hex()for r in rngs]}
                if reference is None:reference=state
                if state!=reference:raise ValueError('PAIRED_SELECTION_WALLET_LIQUIDITY_CLOCK_OR_RNG_CHANGED')
                if reference_full is None:reference_full=full;reference_requests=requests
                if full!=reference_full:
                    if not allow_sized_request_differences:raise ValueError('EXACT_FULL_ORDER_RECORDS_DIFFER_NUMERICAL_MODE_NOT_AUTHORIZED')
                    for case,(left,right)in enumerate(zip(reference_requests,requests)):
                        changed=False
                        for q in left:
                            if left[q]!=right[q]:
                                changed=True
                                remaining=envs[case].api_orders[q].remaining
                                if min(left[q],right[q])<=remaining:raise ValueError('ONLY_ALREADY_SIZED_REQUEST_DIFFERENCES_MAY_BE_REPORTED')
                                request_differences.append({'mode':mode,'case':case,'order':q,'serialRequested':left[q],
                                    'otherRequested':right[q],'sameExecutableRemaining':remaining})
                        if full[case]!=reference_full[case]and not changed:
                            raise ValueError('UNEXPLAINED_FULL_FINANCIAL_RECORD_DIFFERENCE')
                measurements.append({'mode':mode,'seconds':seconds,'decisions':sum(counts),
                    'entityCounts':[len(e.observe()['entities'])for e in envs],
                    'simulatedPrimingReads':[e._benchmark_priming_read_count for e in envs],
                    'sourceWarmupVirtualMs':[e._benchmark_source_warmup_ms for e in envs],
                    'deliveredBooks':[len(e.actor_books)for e in envs]})
                release_worlds(envs);del envs,ledgers,streams,batch
        from statistics import median
        from moloch.learning_v2.train_inner import fingerprint
        if fingerprint(model)!=original_parameters:raise ValueError('BENCHMARK_POLICY_CHANGED')
        medians={mode:median(r['seconds']for r in measurements if r['mode']==mode)for mode in modes}
        result={'classification':'finite_inspected_selection_compute_comparison_not_roi',
            'device':device,'torchVersion':torch.__version__,'checkpointSha256':checkpoint['sha256'],
            'parameterSha256':original_parameters,'datasetManifestSha256':x.dataset_sha,
            'architecture':model.architecture,'parameters':sum(p.numel()for p in model.parameters()),
            'stepsPerCase':steps,'cases':len(descriptors),'measurements':measurements,'medianSeconds':medians,
            'speedupHeadsVersusSerial':medians['serial']/medians['batch_heads'],
            'speedupHeadsVersusBatchEncoding':medians['batch_encoding']/medians['batch_heads'],
            'identicalFinanceLiquidityClocksAndCaseRng':not request_differences,'policyUnchanged':True,
            'identicalCashLotsFillsLiquidityClocksBudgetsAndExecutableRemainders':True,
            'fullOrderRecordsIdentical':not request_differences,'sizedRequestQuantityDifferences':request_differences,
            'allowSizedRequestDifferences':allow_sized_request_differences,
            'recurrentStatesMatchTolerance':True,
            'exactDiscreteActionsStatusesAndBudgets':True,'maxActionFractionAbsoluteError':max_fraction_error,
            'computeCalibration':calibration,'cudaPeakAllocatedBytes':torch.cuda.max_memory_allocated()if device.startswith('cuda')else 0,
            'accountRequests':0,'independentTestUsed':False,'productionChanged':False,'roiEstablished':False,
            'realAccountRequests':0,'simulatedPrimingReadsPerCase':prime_books,
            'fullEpisodeEvaluationCompleted':False,'runtimeSourceSha256':x.frozen['source_sha256']}
        frozen(Path(output)/'RESULT.json',result);return result
    finally:x.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--config',type=Path,required=True)
    p.add_argument('--checkpoint',type=Path,required=True);p.add_argument('--checkpoint-sha256',required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--steps',type=int,default=64)
    p.add_argument('--prime-books',type=int,default=0)
    p.add_argument('--allow-sized-request-differences',action='store_true')
    args=p.parse_args();r=compare(json.loads(args.config.read_text()),
        {'path':str(args.checkpoint),'sha256':args.checkpoint_sha256},args.output,steps=args.steps,prime_books=args.prime_books,
        allow_sized_request_differences=args.allow_sized_request_differences)
    print(json.dumps({k:r[k]for k in('device','medianSeconds','speedupHeadsVersusSerial','speedupHeadsVersusBatchEncoding')}),flush=True)
