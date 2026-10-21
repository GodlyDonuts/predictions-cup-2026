"""Interleaved exact-trajectory CPU replay comparison; no optimizer or test role."""
import argparse,gc,hashlib,json,os,sys,time
from datetime import datetime,timezone
from pathlib import Path

def main(job):
    job=Path(job).resolve();plan=json.loads((job/'PLAN.json').read_text(encoding='utf-8'))
    source=Path(plan['sourceRoot']);sys.path.insert(0,str(source));os.chdir(source)
    from moloch.learning_actions.continue_population import verify_source
    from moloch.learning_actions.population_handoff import sha_file
    from moloch.learning_actions.experiment import Experiment
    from moloch.learning_actions.training import load_checkpoint
    from moloch.learning_actions.stream import Stream
    from moloch.learning_actions.episode_training import release_worlds
    from moloch.learning_actions.episode_tape import financial_digest
    from moloch.learning_v2.train_inner import fingerprint
    import numpy as np
    import torch
    assert verify_source(source)['commit']==plan['sourceCommit']
    assert sha_file(source/'source-snapshot.json')==plan['sourceManifestSha256']
    for name,key in [('config.json','configurationSha256'),('actor-calibration.json','actorCalibrationSha256')]:
        assert sha_file(job/name)==plan[key]
    assert plan['role']=='train' and plan['decisionCap']is None
    owner=Experiment(json.loads((job/'config.json').read_text(encoding='utf-8')),job/'experiment')
    assert owner.dataset_sha==plan['datasetManifestSha256']
    start,end=plan['boundsMs'];assert owner.train_bounds[0]<=start<end<=owner.train_bounds[1]
    calibration=json.loads((job/'actor-calibration.json').read_text(encoding='utf-8'))
    checkpoint=plan['policies']['founder'];results=[];env=None
    def update(digest,value):
        if isinstance(value,np.ndarray):
            digest.update(str(value.dtype).encode());digest.update(repr(value.shape).encode());digest.update(value.tobytes())
        elif isinstance(value,dict):
            for key in sorted(value):update(digest,key);update(digest,value[key])
        elif isinstance(value,(list,tuple)):
            digest.update(str(type(value)).encode())
            for part in value:update(digest,part)
        else:digest.update(repr(value).encode()+b'\0')
    try:
        # Cold source construction is outside each collection timing. Both
        # modes run in both positions to limit first-use/cache attribution.
        for mode in ('defensive_copy','owned_response','owned_response','defensive_copy'):
            model,bundle=load_checkpoint(checkpoint['path'],checkpoint['sha256'],'cpu')
            assert bundle['split_specification_sha256']==plan['originalExperimentSha256']
            assert bundle['dataset_manifest_sha256']==owner.dataset_sha
            del bundle;before=fingerprint(model);model.eval();torch.manual_seed(plan['seed'])
            env=owner.world('train',start,end);owned_targets=len(env.owned_history_payloads)
            assert owned_targets>0
            if mode=='defensive_copy':env.owned_history_payloads.clear()
            observation_hash=hashlib.sha256();actions_hash=hashlib.sha256()
            observe=env.observe
            def observed():
                value=observe();update(observation_hash,value);return value
            env.observe=observed
            stream=Stream(env,model,calibration=calibration,actions_only=True)
            began=time.perf_counter();last=began
            while env.now<env.end:
                update(actions_hash,stream.step())
                if time.perf_counter()-last>20:
                    temp=job/'progress.pending';temp.write_text(json.dumps(dict(mode=mode,case=len(results),
                        decisions=stream.decisions,virtualMs=env.now,endMs=end)),encoding='utf-8');temp.replace(job/'progress.json')
                    last=time.perf_counter()
            elapsed=time.perf_counter()-began
            assert fingerprint(model)==before
            receipts_hash=hashlib.sha256();update(receipts_hash,(list(env.feed_requests),list(env.feed_receipts),
                env.feed_request_counts,env.feed_receipt_counts,env.known_historical_information))
            value=dict(mode=mode,wallSeconds=elapsed,decisions=stream.decisions,ownedTargets=owned_targets,
                observationSha256=observation_hash.hexdigest(),actionsSha256=actions_hash.hexdigest(),
                requestReceiptSha256=receipts_hash.hexdigest(),financialSha256=financial_digest(env),
                fills=len(env.api_fills),feedRequestCounts=env.feed_request_counts,weightsUnchanged=True)
            if results:
                for key in ('decisions','observationSha256','actionsSha256','requestReceiptSha256','financialSha256','fills'):
                    assert value[key]==results[0][key],key
            results.append(value)
            (job/('case-'+str(len(results))+'.json')).write_text(json.dumps(value,indent=2),encoding='utf-8')
            env.observe=observe;release_worlds([env]);env=None;del model,stream;gc.collect()
        owner.close();verify_source(source)
        from statistics import median
        old=median(r['wallSeconds']for r in results if r['mode']=='defensive_copy')
        new=median(r['wallSeconds']for r in results if r['mode']=='owned_response')
        result=dict(observedUtc=datetime.now(timezone.utc).isoformat(),sourceCommit=plan['sourceCommit'],
            planSha256=sha_file(job/'PLAN.json'),boundsMs=plan['boundsMs'],results=results,speedup=old/new,
            checkpointSha256=checkpoint['sha256'],actorCalibrationSha256=plan['actorCalibrationSha256'],
            originalExperimentSha256=plan['originalExperimentSha256'],datasetManifestSha256=owner.dataset_sha,
            equivalent=True,role='train',optimizerRun=False,independentTestUsed=False,productionChanged=False,
            interpretation='interleaved CPU collection with hashing alongside existing GPU training, not end-to-end training or ROI')
        (job/'RESULT.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
        print(json.dumps(dict(speedup=old/new,decisions=results[0]['decisions'],equivalent=True)),flush=True)
    finally:
        if env is not None:release_worlds([env])
        owner.close()

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--job',required=True);args=parser.parse_args()
    try:main(args.job)
    except BaseException as error:
        (Path(args.job)/'FAILED.json').write_text(json.dumps(dict(type=type(error).__name__,reason=str(error))),encoding='utf-8')
        raise
