"""Finite complete-episode learning capacity, never trading/ROI evidence.

Run after the existing learner has released the device, not alongside it.
The original dataset/actor dimensions remain; only development duration shrinks.
"""
import argparse,json,time
from pathlib import Path


def run(config,output,*,episode_ms=2000):
    from .gpu_diagnostic_guard import require_idle_gpu
    require_idle_gpu(config.get('device','cuda'),allow_verified_desktop=True)
    import torch
    from .experiment import Experiment
    from .episode_tape import EpisodeReader
    from .selection import frozen
    from .population_handoff import sha_file
    from moloch.learning_v2.train_inner import fingerprint
    if config.get('mode')!='inspected_development'or config.get('closed_episode_return_replay')is not True:
        raise ValueError('EXPLICIT_CLOSED_DEVELOPMENT_CAPACITY_PROFILE_REQUIRED')
    if type(episode_ms)is not int or episode_ms<=0:raise ValueError('POSITIVE_FINITE_CAPACITY_EPISODE')
    config=dict(config,development_episode_ms=episode_ms,training_snapshot_every_updates=0)
    x=Experiment(config,output)
    try:
        m=x.make_policy();optimizer=x.make_optimizer(m);before=fingerprint(m)
        if x.device.startswith('cuda'):torch.cuda.reset_peak_memory_stats()
        started=time.perf_counter();training=x.train_parallel(m,optimizer);elapsed=time.perf_counter()-started
        directory=Path(output)/'closed-episodes/cohort-000000';complete=directory/'COMPLETE.json'
        reader=EpisodeReader(directory,sha_file(complete))
        try:metadata=reader.metadata
        finally:reader.close()
        changed=fingerprint(m)!=before
        if not changed or x.total_updates<=0:raise ValueError('REAL_CLOSED_EPISODE_OPTIMIZER_UPDATE_REQUIRED')
        if not metadata['complete']or metadata['decisions']!=x.total_decisions:
            raise ValueError('COMPLETE_RECONSTRUCTED_CAPACITY_EPISODES_REQUIRED')
        result={'classification':'finite_closed_episode_gpu_capacity_not_roi'if x.device.startswith('cuda')else
                    'finite_closed_episode_cpu_integration_not_gpu_capacity_or_roi',
            'device':str(next(m.parameters()).device),'parameters':sum(p.numel()for p in m.parameters()),
            'worlds':len(metadata['world_bounds']),'episodeMs':episode_ms,'decisions':x.total_decisions,
            'acceptedUpdates':x.total_updates,'optimizerChangedWeights':changed,'fullReturnTargets':True,
            'sealedEpisodeSha256':sha_file(complete),'sourcePins':x.frozen['source_sha256'],
            'datasetManifestSha256':x.dataset_sha,'elapsedSeconds':elapsed,
            'peakCudaAllocatedBytes':torch.cuda.max_memory_allocated()if x.device.startswith('cuda')else 0,
            'financialResultsAreCapacityOnly':training,'independentTestUsed':False,'productionChanged':False,
            'profitabilityEstablished':False}
        frozen(Path(output)/'RESULT.json',result);return result
    finally:x.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--config',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--episode-ms',type=int,default=2000);a=p.parse_args()
    r=run(json.loads(a.config.read_text(encoding='utf-8-sig')),a.output,episode_ms=a.episode_ms)
    print(json.dumps({k:r[k]for k in('classification','decisions','acceptedUpdates','optimizerChangedWeights')}))
