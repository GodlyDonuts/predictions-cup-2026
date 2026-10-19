"""Fresh raw-API GPU population training; never an exchange client.

Full runs require verified history and frozen chronological roles. Explicit
development runs may exercise short inspected tapes but cannot open a final
test or claim the requested daily-growth qualification. JSONL progress is
append-only; checkpoints are immutable and no old run is silently resumed.
"""
import argparse,copy,hashlib,json,math,random,time
from dataclasses import replace
from pathlib import Path
import torch
from moloch.learning_flow.episodes import FlowTape
from moloch.learning_realism.fast_replay import open_prepared_replay
from moloch.learning_realism.protocol import Split,DAY,digest
from moloch.learning_v2.run_experiment import load_mechanics
from .engine import ApiTape
from .policy import ApiPolicy
from .stream import Stream,calibrate
from .training import collect_chunk,update,save_checkpoint,load_checkpoint
from .economics import GrowthLedger
from .selection import frozen,IndependentTest


class Experiment:
    def __init__(self,config,output):
        self.config=config;self.output=Path(output)
        self.output.mkdir(parents=True,exist_ok=False)
        self.development=config['mode']=='inspected_development'
        if config['mode']not in('inspected_development','full_history'):
            raise ValueError('EXPLICIT_EXPERIMENT_MODE_REQUIRED')
        self.device=config.get('device','cuda');self.seed=config.get('seed',190019)
        torch.set_num_threads(config.get('cpu_threads',2));torch.manual_seed(self.seed)
        self.rng=random.Random(self.seed)
        self.model_size=config.get('hidden_size',4096)
        self.mechanics=load_mechanics(Path(config['mechanics']))
        if self.development:
            self.original=FlowTape(Path(config['dataset']))
            self.split=Split(self.original.times[0],self.original.times[-1]+9*DAY)
            self.gap=0
            a,b=config['development_train_ms'];c,d=config['development_selection_ms']
            if not self.original.times[0]<=a<b<=c<d<=self.original.times[-1]:
                raise ValueError('NONOVERLAPPING_INSPECTED_DEVELOPMENT_WINDOWS_REQUIRED')
            self.train_bounds=(a,b);self.evolution_bounds=(c,d)
        else:
            self.original,_=open_prepared_replay(config['specification'],config['dataset'],
                                                config['mechanics'],'train')
            self.split=self.original.split
            spec=json.loads(Path(config['specification']).read_text())
            self.gap=spec['maximum_venue_gap_ms']
            self.train_bounds=self.split.bounds('train');self.evolution_bounds=self.split.bounds('evolution')
        self.dataset_sha=self.original.manifest_sha256
        # Exact source pins are part of the experiment identity, including
        # simulator, policy, reward, optimizer and role definitions.
        pins={str(p):hashlib.sha256(p.read_bytes()).hexdigest()
              for root in ('learning_actions','learning_realism','learning_v2','learning_flow')
              for p in (Path('moloch')/root).glob('*.py')if not p.name.startswith('test_')}
        self.frozen={'config':config,'dataset_manifest_sha256':self.dataset_sha,
            'mechanics_sha256':hashlib.sha256(Path(config['mechanics']).read_bytes()).hexdigest(),
            'source_sha256':pins,'production_authority':False,
            'full_history_experiment':not self.development,'independent_test_used':False}
        self.spec_sha=frozen(self.output/'experiment.json',self.frozen)
        self.population=[];self.calibration=None;self.total_decisions=0;self.total_updates=0
        self.tape_cache={}

    def log(self,event):
        event={**event,'wall_time_unix':time.time(),'total_decisions':self.total_decisions,
               'total_updates':self.total_updates,'independent_test_used':False}
        with (self.output/'progress.jsonl').open('a',encoding='utf8')as f:
            f.write(json.dumps(event,allow_nan=False)+'\n')
        print(json.dumps(event,allow_nan=False),flush=True)

    def tape(self,role,finalist=None):
        # Inspected development selection stays development; it is not
        # retroactively renamed the human's four-day evolution partition.
        actual='train'if self.development else role
        key=(actual,finalist)
        if key in self.tape_cache:return self.tape_cache[key]
        tape=ApiTape.from_loaded(self.original,split=self.split,role=actual,
                                frozen_finalist_sha256=finalist)
        tape.maximum_boundary_gap_ms=self.gap
        self.tape_cache[key]=tape
        return tape

    def world(self,role,start,end,finalist=None,stress=False):
        mechanics=self.mechanics
        if stress:mechanics=replace(mechanics,depth_fraction_ppm=500000,adverse_execution_micros=5000)
        tape=self.tape(role,finalist)
        earliest=max(tape.times[0],self.split.bounds(tape.role)[0])
        burnin=min(self.config.get('initial_world_history_ms',120000),max(0,start-earliest))
        return tape.episode(mechanics,start,end,
            initial_cash_micros=self.config.get('initial_cash_micros',100000000000),
            target_wealth_micros=self.config.get('target_wealth_micros',1100000000000),burnin_ms=burnin)

    def stream(self,model,env,training=False):
        if self.calibration is None:self.calibration=calibrate(env,model)
        return Stream(env,model,calibration=self.calibration,record_training=training)

    def checkpoint(self,name,model,optimizer):
        path=self.output/(name+'.pt')
        sha=save_checkpoint(path,model,optimizer,dataset_manifest_sha256=self.dataset_sha,
                            split_specification_sha256=self.spec_sha)
        return {'path':str(path),'sha256':sha}

    def train_episode(self,model,optimizer):
        a,b=self.train_bounds
        if self.development:
            length=min(self.config.get('development_episode_ms',120000),b-a)
            start=self.rng.randrange(a,b-length+1);end=start+length
        else:
            # At least two complete days are needed for the supplied sample
            # standard deviation. The calendar window is shared across markets.
            length=self.config.get('training_days',2)*DAY
            if length<2*DAY or length>b-a:raise ValueError('FULL_DAILY_TRAINING_HISTORY_REQUIRED')
            start=self.rng.randrange(a,b-length+1);end=start+length
        env=self.world('train',start,end);stream=self.stream(model,env,True)
        ledger=GrowthLedger(env,development=self.development,
            historical_last_mark=self.config.get('development_last_observed_marks',False),
            maximum_mark_age_ms=self.config.get('maximum_mark_age_ms',120000))
        final=None;reports=[]
        while env.now<end:
            chunk=collect_chunk(stream,self.config.get('compute_chunk_decisions',32),reward_ledger=ledger)
            rewards=[f['economic_reward']for f in chunk.frames]
            reports=update(model,optimizer,chunk,rewards,role='train',epochs=self.config.get('ppo_epochs',1))
            self.total_updates+=sum(r['accepted']for r in reports)
            self.total_decisions+=len(chunk.frames)
            final=chunk.frames[-1].get('economic_result')
            self.log({'event':'train_chunk','virtual_ms':env.now,'episode_end_ms':end,
                'decisions':len(chunk.frames),'chunk_reward':sum(rewards),'updates':reports,
                'fills':len(env.api_fills),'cuda_peak_bytes':torch.cuda.max_memory_allocated()if self.device.startswith('cuda')else 0})
        if final is None:raise ValueError('COMPLETE_ECONOMIC_EPISODE_RESULT_REQUIRED')
        return {**final,'family_counts':dict(stream.action_counts),'status_counts':dict(stream.status_counts),
            'entity_counts':dict(stream.entity_counts),
            'configured_initial_world_history_ms':self.config.get('initial_world_history_ms',120000)}

    def evaluate(self,model,checkpoint,role='evolution',finalist=None):
        if role=='test'and self.development:raise ValueError('INSPECTED_DEVELOPMENT_HAS_NO_INDEPENDENT_TEST')
        start,end=self.evolution_bounds if role=='evolution'else self.split.bounds('test')
        cases=[]
        for seed_offset,stress in enumerate((False,True)):
            torch.manual_seed(self.seed+90000+seed_offset)
            env=self.world(role,start,end,finalist,stress);stream=self.stream(model,env)
            ledger=GrowthLedger(env,development=self.development,
                historical_last_mark=self.config.get('development_last_observed_marks',False),
                maximum_mark_age_ms=self.config.get('maximum_mark_age_ms',120000))
            while env.now<end:
                stream.run_chunk(self.config.get('compute_chunk_decisions',32),deterministic=False)
                ledger.transition()
            result=ledger.finish();cases.append({**result,'stress':stress,'decisions':stream.decisions,
                'family_counts':dict(stream.action_counts),'status_counts':dict(stream.status_counts),
                'entity_counts':dict(stream.entity_counts)})
        return {'role':'development_selection'if self.development else role,
            'eligible':not self.development and all(c['eligible']for c in cases),
            'checkpoint_sha256':checkpoint['sha256'],'reward':min(c['reward']for c in cases),
            'cases':cases,'marked_roi_min':min(c['marked_roi']for c in cases),
            'source_and_execution_calibration_complete':False,'production_profitability_established':False}

    def run(self):
        members=self.config.get('members',4);cycles=self.config.get('cycles',8)
        if type(members)is not int or members<2 or type(cycles)is not int or cycles<1:
            raise ValueError('POSITIVE_POPULATION_AND_CYCLE_COUNTS_REQUIRED')
        for member in range(members):
            torch.manual_seed(self.seed+member)
            model=ApiPolicy(self.model_size).to(self.device)
            # Initialization changes exploration probability, never legal action
            # support: favor initially short alarms without forbidding long ones.
            if self.config.get('initial_short_alarm_prior',True):
                with torch.no_grad():model.fractions.bias[2]=-3.;model.fractions.bias[3]=9.
            opt=torch.optim.Adam(model.parameters(),lr=self.config.get('learning_rate',1e-5))
            self.population.append(self.checkpoint('founder-%03d'%member,model,opt))
            del model,opt
        best=None
        for cycle in range(cycles):
            ranking=[]
            for member,previous in enumerate(self.population):
                torch.manual_seed(self.seed+cycle*1000+member)
                model,bundle=load_checkpoint(previous['path'],previous['sha256'],self.device)
                if bundle['split_specification_sha256']!=self.spec_sha:raise ValueError('FOREIGN_EXPERIMENT_CHECKPOINT')
                opt=torch.optim.Adam(model.parameters(),lr=self.config.get('learning_rate',1e-5))
                opt.load_state_dict(bundle['optimizer']);del bundle
                training=self.train_episode(model,opt)
                checkpoint=self.checkpoint('cycle-%03d-member-%03d'%(cycle,member),model,opt)
                result=self.evaluate(model,checkpoint)
                row={'member':member,'cycle':cycle,'checkpoint':checkpoint,'training':training,'selection':result}
                frozen(self.output/('result-%03d-%03d.json'%(cycle,member)),row)
                ranking.append(row);self.population[member]=checkpoint
                self.log({'event':'member_evaluated',**row})
                del model,opt
            ranking.sort(key=lambda x:x['selection']['reward'],reverse=True)
            if best is None or ranking[0]['selection']['reward']>best['selection']['reward']:best=ranking[0]
            # Retain the champion; replace the weakest member with a mutated
            # copy and a reset optimizer (never inherit stale donor moments).
            if cycle+1<cycles:
                donor=ranking[0]['checkpoint'];loser=ranking[-1]['member']
                model,donor_bundle=load_checkpoint(donor['path'],donor['sha256'],self.device)
                del donor_bundle
                torch.manual_seed(self.seed+cycle+70000)
                with torch.no_grad():
                    for p in model.parameters():p.add_(torch.randn_like(p)*self.config.get('mutation_std',1e-4))
                opt=torch.optim.Adam(model.parameters(),lr=self.config.get('learning_rate',1e-5))
                self.population[loser]=self.checkpoint('offspring-%03d'%cycle,model,opt)
                del model,opt
            self.log({'event':'cycle_complete','cycle':cycle,'best_selection_reward':best['selection']['reward']})
        summary={'classification':'inspected_development_only'if self.development else 'frozen_daily_growth_experiment',
            'best':best,'total_decisions':self.total_decisions,'total_accepted_updates':self.total_updates,
            'independent_test_used':False,'production_changed':False,
            'full_history_qualification_established':False,'production_profitability_established':False}
        if not self.development and self.config.get('test_finalist_once',False)and best['selection']['reward']>0:
            # Changing source, dataset compilation or hyperparameters must
            # not buy another look at the same held-out calendar.
            holdout_identity=digest({'competition':'SIG_Predictions_Cup_2026',
                                      'test_bounds_ms':self.split.bounds('test')})
            registry=IndependentTest(self.config['test_registry'],holdout_identity)
            cp=best['checkpoint'];registry.freeze(cp['path'],cp['sha256'],best['selection'])
            registry.reserve(cp['sha256'])  # consume BEFORE loading test observations
            model,_=load_checkpoint(cp['path'],cp['sha256'],self.device)
            report=self.evaluate(model,cp,'test',cp['sha256']);registry.finish(report)
            summary['independent_test_used']=True;summary['independent_test']=report
        frozen(self.output/'SUMMARY.json',summary);return summary


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--config',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    args=p.parse_args();config=json.loads(args.config.read_text(encoding='utf-8-sig'))
    try:
        experiment=Experiment(config,args.output);summary=experiment.run()
        print(json.dumps({'completed':True,'summary':str(args.output/'SUMMARY.json'),
                          'accepted_updates':summary['total_accepted_updates']}),flush=True)
    except Exception as exc:
        if args.output.exists():
            frozen(args.output/'FAILED.json',{'exception':type(exc).__name__,'reason':str(exc),
                'production_changed':False,'profitability_established':False})
        raise


if __name__=='__main__':main()
