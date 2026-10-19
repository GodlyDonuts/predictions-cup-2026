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
from moloch.learning_realism.protocol import Split,DAY,digest,independent_test_claim
from moloch.learning_v2.run_experiment import load_mechanics
from .engine import ApiTape
from .policy import ApiPolicy
from .stream import Stream,calibrate
from .training import collect_chunk,update,save_checkpoint,load_checkpoint
from .economics import GrowthLedger
from .selection import frozen,IndependentTest,DataUse


class Experiment:
    def __init__(self,config,output):
        self.config=config;self.output=Path(output)
        self.output.mkdir(parents=True,exist_ok=False)
        self.development=config['mode']=='inspected_development'
        if config['mode']not in('inspected_development','full_history'):
            raise ValueError('EXPLICIT_EXPERIMENT_MODE_REQUIRED')
        self.device=config.get('device','cuda');self.seed=config.get('seed',190019)
        if 'cuda_memory_fraction'in config:
            fraction=config['cuda_memory_fraction']
            if type(fraction)not in(int,float)or not 0<fraction<=1:raise ValueError('VALID_CUDA_ALLOCATION_FRACTION')
            if self.device.startswith('cuda'):torch.cuda.set_per_process_memory_fraction(fraction)
        torch.backends.cuda.matmul.allow_tf32=config.get('allow_tf32',False)
        torch.backends.cudnn.allow_tf32=config.get('allow_tf32',False)
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
            'full_history_experiment':not self.development,'independent_test_used':False,
            'actual_split':{'competition_start_ms':self.split.competition_start_ms,'cutoff_ms':self.split.cutoff_ms},
            'actual_train_bounds_ms':list(self.train_bounds),'actual_evolution_bounds_ms':list(self.evolution_bounds),
            'independent_test_source_claim':not self.development and independent_test_claim(self.original.manifest,self.split)}
        if not self.development:
            self.frozen['replay_specification_sha256']=hashlib.sha256(Path(config['specification']).read_bytes()).hexdigest()
        self.spec_sha=frozen(self.output/'experiment.json',self.frozen)
        self.data_use=DataUse()
        # Register before collection, conservatively covering every training
        # and selection time the run may use, across all compiled versions.
        self.data_use.record('train',*self.train_bounds,self.spec_sha)
        self.data_use.record('selection',*self.evolution_bounds,self.spec_sha)
        self.population=[];self.calibration=None;self.total_decisions=0;self.total_updates=0
        self.last_snapshot_bucket=0
        self.tape_cache={}
        self.search_config=config.get('population_search')
        self.archive=None
        if self.search_config is not None:
            from .population import ResearchArchive,island_replacements
            members=config.get('members',4)
            island_replacements([{'member':i,'selection':{'reward':0.}}for i in range(members)],
                members=members,islands=self.search_config['islands'],cycle=0,rng=random.Random(self.seed))
            self.archive=ResearchArchive(self.dataset_sha,self.spec_sha,self.search_config.get('archive_size',32))

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
        key=(role,finalist)
        if key in self.tape_cache:return self.tape_cache[key]
        tape=ApiTape.from_loaded(self.original,split=self.split,role=actual,
                                frozen_finalist_sha256=finalist)
        tape.maximum_boundary_gap_ms=self.gap
        tape.counterparty_replay_enabled=self.config.get('passive_flow_mode')is not None
        tape.counterparty_information_bounds=(self.train_bounds if role=='train'else
            self.evolution_bounds if role=='evolution'else self.split.bounds('test'))
        if self.config.get('snapshot_world_clock')is not None:
            from .snapshot_clock import MODE,prepare
            if self.config['snapshot_world_clock']!=MODE:raise ValueError('EXPLICIT_SNAPSHOT_WORLD_CLOCK_MODE')
            prepare(tape,tape.counterparty_information_bounds,
                role_prefix_initial_state=self.config.get('role_prefix_initial_state',False))
        elif self.config.get('role_prefix_initial_state',False):
            raise ValueError('ROLE_PREFIX_REQUIRES_OBSERVED_SNAPSHOT_WORLD')
        self.tape_cache[key]=tape
        return tape

    def world(self,role,start,end,finalist=None,stress=False):
        mechanics=self.mechanics
        if stress:mechanics=replace(mechanics,depth_fraction_ppm=500000,adverse_execution_micros=5000)
        tape=self.tape(role,finalist)
        earliest=max(tape.times[0],self.split.bounds(tape.role)[0])
        if getattr(tape,'snapshot_world_mode',None)is not None:
            earliest=max(earliest,tape.snapshot_world_information_bounds[0])
        burnin=min(self.config.get('initial_world_history_ms',120000),max(0,start-earliest))
        env=tape.episode(mechanics,start,end,
            initial_cash_micros=self.config.get('initial_cash_micros',100000000000),
            target_wealth_micros=self.config.get('target_wealth_micros',1100000000000),burnin_ms=burnin)
        notifications=self.config.get('private_fill_notifications','legacy_implicit_broker')
        if notifications not in('legacy_implicit_broker','requires_active_sig_stream'):
            raise ValueError('EXPLICIT_PRIVATE_RECEIPT_MODEL_REQUIRED')
        env.private_fill_push=notifications=='legacy_implicit_broker'
        if self.config.get('passive_flow_mode')is not None:
            from .passive import StrictPriorityFlow,DisplayedQueueFlow
            implementations={c.mode:c for c in(StrictPriorityFlow,DisplayedQueueFlow)}
            implementation=implementations.get(self.config['passive_flow_mode'])
            if implementation is None:raise ValueError('EXPLICIT_PASSIVE_HYPOTHESIS_REQUIRED')
            tournaments={r['tournament_id']for r in tape.flow_rows}
            if len(tournaments)!=1:raise ValueError('ONE_SOURCE_BOUND_TOURNAMENT_FLOW_REQUIRED')
            env.passive=implementation(env,next(iter(tournaments)),
                excluded_match_ids=self.config.get('excluded_original_own_match_ids',()),
                exclusion_source_sha256=self.config.get('own_match_exclusion_source_sha256'))
        if self.config.get('sig_realtime_mode')is not None:
            from .realtime import DerivedStream
            if self.config['sig_realtime_mode']!=DerivedStream.mode:raise ValueError('EXPLICIT_DERIVED_STREAM_ASSUMPTION_REQUIRED')
            env.realtime=DerivedStream(env);env.keys['SIG'].append(('realtime_token',))
            initial=self.config.get('initial_sig_realtime')
            if initial not in(None,'request_token'):raise ValueError('EXPLICIT_INITIAL_STREAM_STATE')
            if initial=='request_token':env.realtime.mint()
        return env

    def stream(self,model,env,training=False):
        if self.calibration is None:
            self.calibration=calibrate(env,model)
            frozen(self.output/'compute-calibration.json',self.calibration)
        return Stream(env,model,calibration=self.calibration,record_training=training,
                      behavior_guard=self.config.get('on_policy_guard','sha256'))

    def make_optimizer(self,model,rate=None,entropy=None):
        entropy=self.config.get('entropy_coefficient',.01)if entropy is None else entropy
        if type(entropy)not in(int,float)or not math.isfinite(entropy)or entropy<0:raise ValueError('VALID_LEARNING_ENTROPY_COEFFICIENT')
        opt=torch.optim.Adam(model.parameters(),lr=self.config.get('learning_rate',1e-5)if rate is None else rate)
        opt.param_groups[0]['tartarus_entropy_coefficient']=entropy
        return opt

    def initialize_alarm_prior(self,model):
        scope=self.config.get('short_alarm_prior_scope','legacy_global')
        if scope=='wait_only':
            from .clock_prior import initialize_wait_only
            return initialize_wait_only(model)
        if scope!='legacy_global':raise ValueError('EXPLICIT_SHORT_ALARM_PRIOR_SCOPE')
        with torch.no_grad():model.fractions.bias[2]=-3.;model.fractions.bias[3]=9.
        return {'operator':'legacy_global_fraction_clock_bias'}

    def training_arguments(self,optimizer):
        return {'checkpoint_encoder':self.config.get('checkpoint_entity_encoder',False),
                'entropy_coefficient':optimizer.param_groups[0].get('tartarus_entropy_coefficient',self.config.get('entropy_coefficient',.01))}

    def checkpoint(self,name,model,optimizer):
        path=self.output/(name+'.pt')
        sha=save_checkpoint(path,model,optimizer,dataset_manifest_sha256=self.dataset_sha,
                            split_specification_sha256=self.spec_sha)
        return {'path':str(path),'sha256':sha}

    def training_snapshot(self,model,optimizer):
        interval=self.config.get('training_snapshot_every_updates',0)
        if type(interval)is not int or interval<0:raise ValueError('NONNEGATIVE_TRAINING_SNAPSHOT_INTERVAL')
        if not interval:return
        bucket=self.total_updates//interval
        if bucket<=self.last_snapshot_bucket:return
        # Two rotating recovery slots bound disk use. Completed candidates and
        # archive checkpoint files remain immutable. A slot is NOT an evaluated
        # candidate or a restoration of the in-progress simulated account.
        root=self.output/'training-recovery';root.mkdir(exist_ok=True)
        temporary=root/('pending-%09d.pt'%self.total_updates)
        sha=save_checkpoint(temporary,model,optimizer,dataset_manifest_sha256=self.dataset_sha,
                            split_specification_sha256=self.spec_sha)
        path=root/('slot-%d.pt'%(bucket%2));temporary.replace(path)
        checkpoint={'path':str(path),'sha256':sha}
        record={'checkpoint':checkpoint,'total_decisions':self.total_decisions,
                'total_updates':self.total_updates,'financial_world_resume_supported':False,
                'selection_or_profitability_result':False,'rotating_recovery_copy':True}
        metadata=root/('pending-%09d.json'%self.total_updates);frozen(metadata,record)
        metadata.replace(path.with_suffix('.json'))
        self.last_snapshot_bucket=bucket
        self.log({'event':'training_weight_snapshot','checkpoint':checkpoint,
                  'episode_completed':False,'financial_world_resume_supported':False,
                  'selection_or_profitability_result':False,'rotating_recovery_copy':True,'retention_slots':2})

    def train_episode(self,model,optimizer):
        if self.config.get('training_worlds',1)>1:return self.train_parallel(model,optimizer)
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
            reports=update(model,optimizer,chunk,rewards,role='train',epochs=self.config.get('ppo_epochs',1),**self.training_arguments(optimizer))
            self.total_updates+=sum(r['accepted']for r in reports)
            self.total_decisions+=len(chunk.frames)
            final=chunk.frames[-1].get('economic_result')
            self.log({'event':'train_chunk','virtual_ms':env.now,'episode_end_ms':end,
                'decisions':len(chunk.frames),'chunk_reward':sum(rewards),'updates':reports,
                'fills':len(env.api_fills),'cuda_peak_bytes':torch.cuda.max_memory_allocated()if self.device.startswith('cuda')else 0})
            self.training_snapshot(model,optimizer)
        if final is None:raise ValueError('COMPLETE_ECONOMIC_EPISODE_RESULT_REQUIRED')
        return {**final,'family_counts':dict(stream.action_counts),'status_counts':dict(stream.status_counts),
            'entity_counts':dict(stream.entity_counts),
            'passive_execution':None if env.passive is None else {
                'mode':env.passive.mode,'source_volume':env.passive.source_volume,
                'intercepted_volume':env.passive.intercepted_volume,'calibrated':False},
            'configured_initial_world_history_ms':self.config.get('initial_world_history_ms',120000)}

    def train_parallel(self,model,optimizer):
        from .batched import BatchStream
        count=self.config['training_worlds']
        if type(count)is not int or count<2:raise ValueError('AT_LEAST_TWO_INDEPENDENT_TRAINING_WORLDS')
        a,b=self.train_bounds
        length=(min(self.config.get('development_episode_ms',120000),b-a)if self.development
                else self.config.get('training_days',2)*DAY)
        if length>b-a or (not self.development and length<2*DAY):raise ValueError('COMPLETE_TRAINING_CALENDAR_REQUIRED')
        starts=[self.rng.randrange(a,b-length+1)for _ in range(count)]
        envs=[self.world('train',start,start+length)for start in starts]
        ledgers=[GrowthLedger(e,development=self.development,
            historical_last_mark=self.config.get('development_last_observed_marks',False),
            maximum_mark_age_ms=self.config.get('maximum_mark_age_ms',120000))for e in envs]
        if self.calibration is None:
            self.calibration=calibrate(envs[0],model)
            frozen(self.output/'compute-calibration.json',self.calibration)
        stream=BatchStream(envs,model,ledgers,calibration=self.calibration,
                           behavior_guard=self.config.get('on_policy_guard','sha256'))
        while any(e.now<e.end for e in envs):
            chunk=stream.collect(self.config.get('compute_chunk_decisions',32))
            rewards=[f['economic_reward']for f in chunk.frames]
            reports=update(model,optimizer,chunk,rewards,role='train',epochs=self.config.get('ppo_epochs',1),**self.training_arguments(optimizer))
            self.total_updates+=sum(r['accepted']for r in reports)
            decisions=sum(int(f['learn'].sum())for f in chunk.frames)
            self.total_decisions+=decisions
            self.log({'event':'parallel_train_chunk','worlds':count,'decisions':decisions,
                'virtual_ms':[e.now for e in envs],'episode_end_ms':[e.end for e in envs],
                'completed_worlds':len(stream.completed),'updates':reports,
                'fills':sum(len(e.api_fills)for e in envs),
                'cuda_peak_bytes':torch.cuda.max_memory_allocated()if self.device.startswith('cuda')else 0})
            self.training_snapshot(model,optimizer)
        cases=[{**stream.completed[i],'family_counts':dict(stream.action_counts[i]),
            'passive_execution':None if envs[i].passive is None else {
                'mode':envs[i].passive.mode,'source_volume':envs[i].passive.source_volume,
                'intercepted_volume':envs[i].passive.intercepted_volume,'calibrated':False},
            'derived_stream':None if envs[i].realtime is None else {
                'mints':envs[i].realtime.mints,'book_deliveries':envs[i].realtime.deliveries}}
            for i in range(count)]
        return {'classification':'independent_simulated_worlds_not_additional_historical_evidence',
            'worlds':count,'reward':sum(c['reward']for c in cases)/count,
            'marked_roi':sum(c['marked_roi']for c in cases)/count,
            'fill_count':sum(c['fill_count']for c in cases),'eligible':False,
            'cases':cases,'all_worlds_preserve_financial_state_through_horizon':True}

    def evaluate(self,model,checkpoint,role='evolution',finalist=None):
        if role=='test'and self.development:raise ValueError('INSPECTED_DEVELOPMENT_HAS_NO_INDEPENDENT_TEST')
        if role=='test'and not independent_test_claim(self.original.manifest,self.split):
            raise ValueError('INDEPENDENT_TEST_SOURCE_PROVENANCE_NOT_ESTABLISHED')
        start,end=self.evolution_bounds if role=='evolution'else self.split.bounds('test')
        if role=='test':self.data_use.record('test',start,end,self.spec_sha)
        blocks=(self.search_config or{}).get('development_selection_blocks',1)
        if type(blocks)is not int or blocks<1 or blocks>end-start:raise ValueError('VALID_DEVELOPMENT_SELECTION_BLOCKS')
        if not self.development and blocks!=1:raise ValueError('FULL_DAILY_GROWTH_EVALUATION_MUST_REMAIN_CONTINUOUS')
        calendars=[(start+(end-start)*i//blocks,start+(end-start)*(i+1)//blocks)for i in range(blocks)]
        cases=[]
        for seed_offset,(case_start,case_end,stress)in enumerate((a,b,s)for a,b in calendars for s in(False,True)):
            torch.manual_seed(self.seed+90000+seed_offset)
            env=self.world(role,case_start,case_end,finalist,stress);stream=self.stream(model,env)
            ledger=GrowthLedger(env,development=self.development,
                historical_last_mark=self.config.get('development_last_observed_marks',False),
                maximum_mark_age_ms=self.config.get('maximum_mark_age_ms',120000))
            while env.now<case_end:
                stream.run_chunk(self.config.get('compute_chunk_decisions',32),deterministic=False)
                ledger.transition()
            result=ledger.finish();cases.append({**result,'stress':stress,'start_ms':case_start,'end_ms':case_end,
                'selection_seed':self.seed+90000+seed_offset,'decisions':stream.decisions,
                'passive_execution':None if env.passive is None else {
                    'mode':env.passive.mode,'source_volume':env.passive.source_volume,
                    'intercepted_volume':env.passive.intercepted_volume,'calibrated':False},
                'family_counts':dict(stream.action_counts),'status_counts':dict(stream.status_counts),
                'entity_counts':dict(stream.entity_counts)})
        if role=='test'and self.data_use.overlaps(start,end,ignore_test=True):
            raise ValueError('TEST_CALENDAR_CONCURRENTLY_USED_FOR_OPTIMIZATION')
        return {'role':'development_selection'if self.development else role,
            'eligible':not self.development and all(c['eligible']for c in cases),
            'checkpoint_sha256':checkpoint['sha256'],'reward':min(c['reward']for c in cases),
            'cases':cases,'marked_roi_min':min(c['marked_roi']for c in cases),
            'source_and_execution_calibration_complete':False,'production_profitability_established':False}

    def mutation_probe(self,model,cycle,slot):
        from .population import freeze_probe
        start,end=self.train_bounds
        env=self.world('train',start,end);stream=self.stream(model,env,training=True);frames=[]
        count=self.search_config.get('probe_decisions',16)
        if type(count)is not int or count<1:raise ValueError('POSITIVE_TRAINING_PROBE_SIZE')
        for _ in range(count):
            row=stream.step(deterministic=True)
            if 'at_ms'not in row:break
            frames.append({**{k:stream.last_frame[k]for k in ('contracts','global_state','present','masks','reset')},
                           'at_ms':row['at_ms']})
            if row.get('terminal'):break
        probe=freeze_probe(frames,role='train',dataset_sha256=self.dataset_sha,bounds=self.train_bounds)
        path=self.output/('mutation-probe-%03d-%03d.pt'%(cycle,slot))
        with path.open('xb')as f:torch.save({'frames':probe.frames,'sha256':probe.sha256,
            'dataset_sha256':probe.dataset_sha256,'bounds':probe.bounds},f)
        return probe,hashlib.sha256(path.read_bytes()).hexdigest()

    def replace_population(self,ranking,cycle):
        from .population import island_replacements,mutate
        replacements=island_replacements(ranking,members=len(self.population),
            islands=self.search_config['islands'],cycle=cycle,rng=self.rng,
            migration_every=self.search_config.get('migration_every',4))
        by_member={r['member']:r for r in ranking}
        for r in replacements:
            donor=by_member[r['donor']]['checkpoint'];slot=r['loser']
            donor_source='current_island_population'
            if r['migration']and self.archive.entries and self.rng.random()<.25:
                archived=self.rng.choice(list(self.archive.entries.values()))
                donor=archived['checkpoint'];donor_source='protected_development_archive'
                r['donor']=archived['member'];r['migration']=r['donor']%self.search_config['islands']!=r['island']
            model,bundle=load_checkpoint(donor['path'],donor['sha256'],self.device)
            if bundle['split_specification_sha256']!=self.spec_sha:raise ValueError('FOREIGN_ISLAND_DONOR')
            donor_rate=bundle['optimizer']['param_groups'][0]['lr']
            donor_entropy=bundle['optimizer']['param_groups'][0].get('tartarus_entropy_coefficient',.01)
            del bundle
            probe,probe_file_sha=self.mutation_probe(model,cycle,slot)
            report=mutate(model,probe,r['operator'],self.seed+cycle*1000+slot+70000,
                fresh_initializer=(self.initialize_alarm_prior if self.config.get('initial_short_alarm_prior',True)else None))
            scales={'local':(.8,1.2),'medium':(.5,2.),'large':(.25,4.),'head_reset':(.5,2.)}
            rate=(self.config.get('learning_rate',1e-5)if r['operator']=='fresh'else
                  min(.001,max(.000001,donor_rate*self.rng.choice(scales[r['operator']]))))
            entropy=(self.rng.choice(self.config.get('initial_entropy_coefficients',[self.config.get('entropy_coefficient',.01)]))
                if r['operator']=='fresh'else min(.1,max(0.,donor_entropy*self.rng.choice(scales[r['operator']]))))
            opt=self.make_optimizer(model,rate,entropy)
            child=self.checkpoint('offspring-%03d-%03d'%(cycle,slot),model,opt)
            self.population[slot]=child
            self.log({'event':'population_replacement',**r,'mutation':report,'checkpoint':child,
                      'donor_checkpoint':donor,'probe_file_sha256':probe_file_sha,
                      'donor_source':donor_source,'parent_learning_rate':donor_rate,'child_learning_rate':rate,
                      'parent_entropy_coefficient':donor_entropy,'child_entropy_coefficient':entropy,
                      'child_receives_full_training_block_before_selection':True})
            del model,opt

    def run(self):
        members=self.config.get('members',4);cycles=self.config.get('cycles',8)
        if type(members)is not int or members<2 or type(cycles)is not int or cycles<1:
            raise ValueError('POSITIVE_POPULATION_AND_CYCLE_COUNTS_REQUIRED')
        inherited=self.config.get('development_initial_checkpoint')
        parents=self.config.get('development_initial_checkpoints')or([inherited]if inherited else[])
        if parents and not self.development:raise ValueError('DEVELOPMENT_WEIGHT_TRANSFER_CANNOT_QUALIFY_FULL_HISTORY')
        fresh=self.config.get('fresh_founders',0)
        if type(fresh)is not int or not 0<=fresh<=members:raise ValueError('VALID_FRESH_LINEAGE_COUNT_REQUIRED')
        for member in range(members):
            torch.manual_seed(self.seed+member)
            inherited=parents[member%len(parents)]if parents and member<members-fresh else None
            if inherited:
                model,parent=load_checkpoint(inherited['path'],inherited['sha256'],self.device)
                from .lineage import development_parent
                lineage=development_parent(parent,inherited['path'],new_dataset_sha256=self.dataset_sha,
                    allow_cross_dataset=self.config.get('development_allow_cross_dataset_transfer',False))
                if lineage is not None:
                    for role,bounds in lineage['prior_calendar_bounds_ms'].items():
                        self.data_use.record('train'if role=='train'else'selection',*bounds,lineage['source_experiment_sha256'])
                    frozen(self.output/('founder-%03d-lineage.json'%member),lineage)
                    if (self.config.get('short_alarm_prior_scope')=='wait_only'and
                        lineage['parent_short_alarm_prior_scope']!='wait_only'):
                        if self.config.get('migrate_shared_short_alarm_prior')is not True:
                            raise ValueError('EXPLICIT_PRIOR_MIGRATION_REQUIRED')
                        from .clock_prior import migrate_shared_prior
                        frozen(self.output/('founder-%03d-clock-migration.json'%member),migrate_shared_prior(model))
                del parent
                if member>=len(parents):
                    with torch.no_grad():
                        for p in model.parameters():p.add_(torch.randn_like(p)*self.config.get('mutation_std',1e-4))
            else:model=ApiPolicy(self.model_size).to(self.device)
            # Initialization changes exploration probability, never legal action
            # support: favor initially short alarms without forbidding long ones.
            if not inherited and self.config.get('initial_short_alarm_prior',True):
                self.initialize_alarm_prior(model)
            entropies=self.config.get('initial_entropy_coefficients',[self.config.get('entropy_coefficient',.01)])
            if not entropies:raise ValueError('NONEMPTY_INITIAL_ENTROPY_COEFFICIENTS')
            opt=self.make_optimizer(model,entropy=entropies[member%len(entropies)])
            self.population.append(self.checkpoint('founder-%03d'%member,model,opt))
            del model,opt
        best=None
        for cycle in range(cycles):
            ranking=[]
            for member,previous in enumerate(self.population):
                torch.manual_seed(self.seed+cycle*1000+member)
                model,bundle=load_checkpoint(previous['path'],previous['sha256'],self.device)
                if bundle['split_specification_sha256']!=self.spec_sha:raise ValueError('FOREIGN_EXPERIMENT_CHECKPOINT')
                opt=self.make_optimizer(model)
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
            if self.archive is not None:
                for row in ranking:self.archive.consider(row)
                frozen(self.output/('research-archive-%03d.json'%cycle),self.archive.json())
            # Opt-in search keeps island champions and applies measured
            # multiscale operators. Legacy runs retain their original operator.
            if cycle+1<cycles and self.search_config is not None:
                self.replace_population(ranking,cycle)
            elif cycle+1<cycles:
                donor=ranking[0]['checkpoint'];loser=ranking[-1]['member']
                model,donor_bundle=load_checkpoint(donor['path'],donor['sha256'],self.device)
                del donor_bundle
                torch.manual_seed(self.seed+cycle+70000)
                with torch.no_grad():
                    for p in model.parameters():p.add_(torch.randn_like(p)*self.config.get('mutation_std',1e-4))
                opt=self.make_optimizer(model)
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
