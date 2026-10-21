"""CPU processes advance evolution cases; the frozen model stays in its owner.

Independent testing keeps the original path and reservation protocol. Selection
uses the original case seeds, virtual latency, paid reads and terminal valuation.
"""
from dataclasses import dataclass
from pathlib import Path
import time
from .economics import GrowthLedger
from .process_worlds import ProcessCohort,WaitingDataUse


@dataclass(frozen=True)
class EvolutionFactory:
    config:dict
    directory:str
    start:int
    end:int
    stress:bool

    def __call__(self):
        from .experiment import Experiment
        config=dict(self.config,device='cpu',cpu_threads=1,
                    process_source_worlds=False,process_evaluation_worlds=False)
        owner=Experiment(config,Path(self.directory),data_use=WaitingDataUse())
        try:
            if not owner.evolution_bounds[0]<=self.start<self.end<=owner.evolution_bounds[1]:
                raise ValueError('PROCESS_EVALUATION_OUTSIDE_EVOLUTION_CALENDAR')
            env=owner.world('evolution',self.start,self.end,stress=self.stress)
            ledger=GrowthLedger(env,development=owner.development,
                development_daily_growth=config.get('development_selection_daily_growth',False),
                historical_last_mark=config.get('development_last_observed_marks',False),
                maximum_mark_age_ms=config.get('maximum_mark_age_ms',120000))
            return env,ledger,owner
        except BaseException:owner.close();raise


def run(owner,model,descriptors,*,batch_heads):
    from .evaluation_batch import EvaluationBatch
    from .episode_training import release_worlds
    from .selection import frozen
    if owner.calibration is None:
        a,b,stress=descriptors[0]
        local=owner.world('evolution',a,b,stress=stress)
        try:owner.stream(model,local)
        finally:release_worlds([local])
    index=getattr(owner,'evaluation_cohort_counter',0);owner.evaluation_cohort_counter=index+1
    directory=owner.output/'evaluation-cohorts'/('cohort-%06d'%index)
    factories=[(EvolutionFactory(owner.config,str(directory/('case-%03d'%i)),a,b,stress),())
               for i,(a,b,stress)in enumerate(descriptors)]
    cohort=ProcessCohort(factories,mode='evaluation')
    try:
        frozen(directory/'worker-identities.json',{'startMethod':'spawn','workers':cohort.identities,
            'worldBounds':[[e.now,e.end]for e in cohort.worlds],
            'modelInParentOnly':True,'independentTestUsed':False,'productionAuthority':False})
        batch=EvaluationBatch(cohort.worlds,model,[owner.seed+90000+i for i in range(len(descriptors))],
            calibration=owner.calibration,batch_action_heads=batch_heads,
            actions_only=owner.config.get('evaluation_actions_only',False),world_backend=cohort)
        last=time.monotonic()
        while any(e.now<e.end for e in cohort.worlds):
            batch.run_chunk(owner.config.get('compute_chunk_decisions',32))
            if time.monotonic()-last>=30:
                owner.log({'event':'evolution_case_progress','computeMode':'spawned_source_worlds',
                    'virtual_ms':[e.now for e in cohort.worlds],
                    'episode_end_ms':[e.end for e in cohort.worlds],'case_decisions':batch.decisions,
                    'case_fills':[e.details['fill_count']for e in cohort.worlds]})
                last=time.monotonic()
        finals=cohort.exchange(cohort.worlds,[('evaluation_finish',None)]*len(cohort.worlds))
        return [{**row['final'],'stress':stress,'start_ms':a,'end_ms':b,
            'selection_seed':owner.seed+90000+i,'decisions':batch.decisions[i],
            'passive_execution':env.details['passive_execution'],
            'family_counts':dict(batch.action_counts[i]),'status_counts':dict(batch.status_counts[i]),
            'entity_counts':dict(batch.entity_counts[i]),
            'valuation_observation_schedule':'start_daily_boundaries_and_terminal',
            'evaluation_compute_mode':('batch_encoding_and_action_heads_case_local_generators'
                if batch_heads else 'batch_encoding_case_local_rng'),
            'source_compute_mode':'spawned_independent_cpu_worlds'}
            for i,(env,row,(a,b,stress))in enumerate(zip(cohort.worlds,finals,descriptors))]
    finally:cohort.close()
