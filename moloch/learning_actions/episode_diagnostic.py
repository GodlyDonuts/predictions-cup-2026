"""Direct-script CPU attribution of exact actions from a sealed TRAIN cohort."""
import argparse,hashlib,json,sys,time,os,traceback
from pathlib import Path
from collections import Counter


class ActionAttribution:
    """Observe replay calls without changing actions, clocks or return values."""
    def __init__(self,env):
        self.env=env;self.original=env.act_raw;self.families=Counter();self.results=Counter()
        self.reads=Counter();self.order_books=Counter();self.stream_states=Counter()
        env.act_raw=self.act

    def act(self,action,observation):
        family=int(action['family']);kind,key=observation['entities'][int(action['market'])]
        self.families[str(family)]+=1
        stream=getattr(self.env,'realtime',None)
        self.stream_states['enabled'if stream is not None and stream.enabled else'disabled']+=1
        if family==10 and kind=='read':
            venue,target=key;self.reads[venue+'/'+target[0]]+=1
        if family in(1,2,5,6)and kind=='contract':
            book=self.env.actor_books.get(key)
            self.order_books['absent'if book is None else'invalid'if book.invalidated else'valid']+=1
        result=self.original(action,observation)
        self.results[str(family)+'/'+result['status']]+=1
        return result

    def summary(self):
        return {'familyCounts':dict(self.families),'familyResultCounts':dict(self.results),
                'readTargetsAttempted':dict(self.reads),'orderBookStateBeforeAction':dict(self.order_books),
                'streamStateBeforeAction':dict(self.stream_states),
                'timing':'cached_state_before_action_after_recorded_compute_latency',
                'requestedReadsAreNotProofOfSuccessfulReceipts':True}

    def close(self):self.env.act_raw=self.original


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in('source-root','run','cohort','output','config'):p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--config-sha256',required=True)
    p.add_argument('--uninstrumented',action='store_true')
    a=p.parse_args()
    if any(k=='moloch'or k.startswith('moloch.')for k in sys.modules):
        raise ValueError('DIRECT_FRESH_PROCESS_REQUIRED')
    root=a.source_root.resolve();run=a.run.resolve();cohort=a.cohort.resolve()
    a.output=a.output.resolve();raw_config=a.config.read_bytes()
    if hashlib.sha256(raw_config).hexdigest()!=a.config_sha256:
        raise ValueError('EXACT_ORIGINAL_CONFIGURATION_BYTES_REQUIRED')
    if not cohort.is_relative_to(run/'closed-episodes'):raise ValueError('EXACT_RUN_COHORT_REQUIRED')
    import torch
    torch.cuda.is_available=lambda:False
    torch.cuda.device_count=lambda:0
    torch.cuda.manual_seed_all=lambda *_:None
    torch.set_num_threads(1);sys.path.insert(0,str(root));os.chdir(root)
    from moloch.learning_actions.continue_population import verify_source
    from moloch.learning_actions.experiment import Experiment
    from moloch.learning_actions.episode_tape import EpisodeReader
    from moloch.learning_actions.episode_training import release_worlds
    from moloch.learning_actions.economics import GrowthLedger
    from moloch.learning_actions.pnl_attribution import Attribution
    source=verify_source(root);original=(run/'experiment.json').read_bytes();parent=json.loads(original)
    if(parent['full_history_experiment']is not False or parent['independent_test_used']is not False
            or parent['production_authority']is not False):raise ValueError('INSPECTED_DEVELOPMENT_ONLY')
    for name,digest in parent['source_sha256'].items():
        if hashlib.sha256((root/name).read_bytes()).hexdigest()!=digest:raise ValueError('EXACT_ORIGINAL_SOURCE_REQUIRED')
    reader=EpisodeReader(cohort,hashlib.sha256((cohort/'COMPLETE.json').read_bytes()).hexdigest())
    if reader.metadata['specification_sha256']!=hashlib.sha256(original).hexdigest():
        raise ValueError('EXACT_ORIGINAL_EPISODE_EXPERIMENT_REQUIRED')
    config=json.loads(raw_config)
    if config!=parent['config']:raise ValueError('ORIGINAL_CONFIGURATION_CONTENTS_REQUIRED')
    config['device']='cpu';x=Experiment(config,a.output)
    envs=[];recorders=[];actions=[];started=time.perf_counter();decisions=0;batches=0
    try:
        if reader.metadata['dataset_sha256']!=x.dataset_sha:raise ValueError('EXACT_ORIGINAL_EPISODE_DATASET_REQUIRED')
        for lo,hi in reader.metadata['world_bounds']:
            if not x.train_bounds[0]<=lo<hi<=x.train_bounds[1]:raise ValueError('TRAIN_COHORT_BOUNDS_REQUIRED')
            env=x.world('train',lo,hi);envs.append(env)
        options={'development':True,'historical_last_mark':config.get('development_last_observed_marks',False),
                 'maximum_mark_age_ms':config.get('maximum_mark_age_ms',120000)}
        ledgers=[GrowthLedger(e,**options)for e in envs]
        if not a.uninstrumented:
            actions=[ActionAttribution(e)for e in envs]
            recorders=[Attribution(e,historical_last_mark=options['historical_last_mark'],
                maximum_mark_age_ms=options['maximum_mark_age_ms'])for e in envs]
        for chunk,_ in reader.replay(envs,ledgers,'cpu'):
            decisions+=sum(int(f['learn'].sum())for f in chunk.frames);batches+=1
        results=[r.summary()for r in recorders]
        report={'classification':'exact_sealed_train_action_replay_not_independent_roi',
            'sourceCommit':source['commit'],'sourceExperimentSha256':hashlib.sha256(original).hexdigest(),
            'originalConfigurationSha256':a.config_sha256,
            'episodeManifestSha256':reader.manifest_sha256,'actorWeightFingerprint':reader.metadata['actor_sha256'],
            'datasetManifestSha256':x.dataset_sha,'decisions':decisions,'batches':batches,
            'wallSeconds':time.perf_counter()-started,'worldCount':len(envs),'worlds':results,
            'actionsByWorld':[v.summary()for v in actions],
            'instrumentationEnabled':not a.uninstrumented,
            'allFrameClockRewardAndFinancialChecksPassed':True,'newPolicyInferenceRun':False,
            'optimizerRun':False,'accountRequests':0,'independentTestUsed':False,'productionChanged':False}
        (a.output/'RESULT.json').write_text(json.dumps(report,indent=2))
        print(json.dumps({'complete':True,'worlds':len(envs),'decisions':decisions,
                         'fills':sum(r['fillCount']for r in results),'wallSeconds':report['wallSeconds']}),flush=True)
    except BaseException as error:
        position={}
        for frame,_ in traceback.walk_tb(error.__traceback__):
            if frame.f_code.co_name=='replay':
                position={k:frame.f_locals.get(k)for k in('batch','step')}
        (a.output/'FAILED.json').write_text(json.dumps({'errorType':type(error).__name__,
            'reason':str(error),'position':position,'instrumentationEnabled':not a.uninstrumented,
            'sourceChecksBypassed':False,'productionChanged':False},indent=2));raise
    finally:
        for v in actions:v.close()
        for r in recorders:r.close()
        release_worlds(envs);reader.close();x.close()


if __name__=='__main__':main()
