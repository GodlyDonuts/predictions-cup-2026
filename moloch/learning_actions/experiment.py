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
from .neural_sessions import neural_section,neural_only,seed_neural
from .economics import GrowthLedger
from .training_reward import horizon as training_horizon,ledger as training_ledger
from .selection import frozen,IndependentTest,DataUse


class Experiment:
    def close(self):
        cohort=getattr(self,'_source_cohort',None)
        if cohort is not None:cohort.close();self._source_cohort=None
        history=getattr(self,'history_bindings',None)
        if history is not None:history.close()
        store=getattr(getattr(self,'original',None),'disk_store',None)
        if store is not None:store.close()

    def __init__(self,config,output,*,data_use=None):
        process_worlds=config.get('process_source_worlds',False)
        if type(process_worlds)is not bool:raise ValueError('EXPLICIT_PROCESS_SOURCE_WORLD_PROFILE')
        if process_worlds and config.get('closed_episode_return_replay',False):
            raise ValueError('PROCESS_SOURCE_WORLDS_CURRENTLY_REQUIRE_ONLINE_PPO')
        batch=config.get('batched_evaluation',False);heads=config.get('batched_evaluation_action_heads',False)
        if type(batch)is not bool or type(heads)is not bool or(heads and not batch):
            raise ValueError('EXPLICIT_BATCH_ACTION_HEADS_REQUIRE_BATCH_EVALUATION')
        self.config=config;self.output=Path(output)
        from .realtime import refresh_profile
        effective_refresh=refresh_profile(config.get('sig_realtime_refresh_mode'))
        if config.get('sig_realtime_refresh_mode')is not None and config.get('sig_realtime_mode')is None:
            raise ValueError('REFRESH_PROFILE_REQUIRES_REALTIME_ADAPTER')
        if type(config.get('evaluation_actions_only',False))is not bool:raise ValueError('EXPLICIT_EVALUATION_STATISTICS_PROFILE')
        if type(config.get('compact_rollout_statistics',False))is not bool:raise ValueError('EXPLICIT_ROLLOUT_STATISTICS_PROFILE')
        self.output.mkdir(parents=True,exist_ok=False)
        self.development=config['mode']=='inspected_development'
        if config['mode']not in('inspected_development','full_history'):
            raise ValueError('EXPLICIT_EXPERIMENT_MODE_REQUIRED')
        self.device=config.get('device','cuda');self.seed=config.get('seed',190019)
        if type(config.get('derived_public_trade_stream',False))is not bool:
            raise ValueError('EXPLICIT_DERIVED_PUBLIC_TRADE_PROFILE')
        if config.get('derived_public_trade_stream',False)and config.get('sig_realtime_mode')is None:
            raise ValueError('PUBLIC_TRADES_REQUIRE_DERIVED_STREAM')
        from .acting_validation import validate as acting_mode,STRICT
        acting_mode(config.get('acting_validation',STRICT))
        if type(config.get('closed_episode_return_replay',False))is not bool:
            raise ValueError('EXPLICIT_CLOSED_EPISODE_TRAINING_PROFILE')
        if type(config.get('closed_episode_continue_after_kl_rejection',False))is not bool:
            raise ValueError('EXPLICIT_CLOSED_EPISODE_REJECTION_PROFILE')
        if config.get('closed_episode_continue_after_kl_rejection',False)and not config.get('closed_episode_return_replay',False):
            raise ValueError('REJECTION_CONTINUATION_REQUIRES_CLOSED_EPISODES')
        from .episode_tape import observation_digest
        observation_digest(config.get('episode_observation_digest','legacy'))
        if config.get('episode_observation_digest','legacy')!='legacy'and not config.get('closed_episode_return_replay',False):
            raise ValueError('PACKED_EPISODE_DIGEST_REQUIRES_CLOSED_EPISODES')
        with neural_section():
            if 'cuda_memory_fraction'in config:
                fraction=config['cuda_memory_fraction']
                if type(fraction)not in(int,float)or not 0<fraction<=1:raise ValueError('VALID_CUDA_ALLOCATION_FRACTION')
                if self.device.startswith('cuda'):torch.cuda.set_per_process_memory_fraction(fraction)
            torch.backends.cuda.matmul.allow_tf32=config.get('allow_tf32',False)
            torch.backends.cudnn.allow_tf32=config.get('allow_tf32',False)
            torch.set_num_threads(config.get('cpu_threads',2));seed_neural(self.seed)
        self.rng=random.Random(self.seed)
        self.model_size=config.get('hidden_size',4096)
        self.mechanics=load_mechanics(Path(config['mechanics']))
        disk_original=None
        if config.get('replay_cache')is not None:
            from .disk_tape import DiskFlowTape
            cache=config['replay_cache']
            disk_original=DiskFlowTape(cache['path'],cache['manifest_sha256'],config['dataset'],
                decoder_audit=cache.get('decoder_audit'),decoder_audit_sha256=cache.get('decoder_audit_sha256'))
        if self.development:
            self.original=disk_original if disk_original is not None else FlowTape(Path(config['dataset']))
            self.split=Split(self.original.times[0],self.original.times[-1]+9*DAY)
            self.gap=0
            a,b=config['development_train_ms'];c,d=config['development_selection_ms']
            if not self.original.times[0]<=a<b<=c<d<=self.original.times[-1]:
                raise ValueError('NONOVERLAPPING_INSPECTED_DEVELOPMENT_WINDOWS_REQUIRED')
            self.train_bounds=(a,b);self.evolution_bounds=(c,d)
        else:
            self.original,_=open_prepared_replay(config['specification'],config['dataset'],
                                                config['mechanics'],'train',original=disk_original)
            self.split=self.original.split
            spec=json.loads(Path(config['specification']).read_text())
            self.gap=spec['maximum_venue_gap_ms']
            self.train_bounds=self.split.bounds('train');self.evolution_bounds=self.split.bounds('evolution')
        training_horizon(config,self.train_bounds,development=self.development)
        self.dataset_sha=self.original.manifest_sha256
        from .development_calendars import calendars
        calendars(*self.evolution_bounds,(config.get('population_search')or{}).get('development_selection_blocks',1),
            development=self.development,episode_ms=config.get('development_selection_episode_ms'))
        from .state_encoding import validate
        validate(config.get('global_state_encoding','raw'))
        from .observation_protocol import validate as protocol,CURRENT,QUERY_WINDOW,SCOPED_EXECUTION,MEMORY_JOINT,ORDER_PAGES,COMPONENT_DEBT,BULK_QUOTES,API_TARGETS,HISTORICAL_INFORMATION
        windows=config.get('trade_query_windows',False)
        if type(windows)is not bool or(windows and config.get('full_depth_observations')is not True):
            raise ValueError('QUERY_WINDOWS_REQUIRE_EXPLICIT_FULL_DEPTH_PROFILE')
        scoped=config.get('scoped_execution_receipts',False)
        if type(scoped)is not bool or(scoped and not windows):raise ValueError('SCOPED_EXECUTION_REQUIRES_QUERY_PROFILE')
        joint=config.get('joint_local_memory_density',False)
        if type(joint)is not bool or(joint and not scoped):raise ValueError('JOINT_MEMORY_REQUIRES_SCOPED_QUERY_PROFILE')
        pages=config.get('owned_order_pages',False)
        if type(pages)is not bool or(pages and not joint):raise ValueError('ORDER_PAGES_REQUIRE_JOINT_SCOPED_QUERY_PROFILE')
        debt=config.get('component_debt_accounting',False)
        if type(debt)is not bool or(debt and not pages):raise ValueError('COMPONENT_DEBT_REQUIRES_ORDER_PAGE_PROFILE')
        quotes=config.get('bulk_quote_observations',False)
        if type(quotes)is not bool or(quotes and not debt):raise ValueError('BULK_QUOTES_REQUIRE_COMPONENT_SCOPED_PROFILE')
        named=config.get('api_target_affordances',False)
        if type(named)is not bool or(named and not quotes):raise ValueError('API_LABELS_REQUIRE_EXPLICIT_BULK_PROFILE')
        history=config.get('historical_information')
        from .historical_memory import profile as memory_profile
        if memory_profile(config.get('historical_memory_profile'))is not None and history is None:
            raise ValueError('HISTORICAL_MEMORY_REQUIRES_EXPLICIT_HISTORY_BINDING')
        self.history_bindings=None
        if history is not None:
            if not named:raise ValueError('HISTORICAL_INFORMATION_REQUIRES_NAMED_API_PROFILE')
            if (config.get('development_initial_checkpoint')or config.get('development_initial_checkpoints')) and (
                    not self.development or config.get('historical_development_weight_transfer') is not True):
                raise ValueError('EXPLICIT_HISTORICAL_DEVELOPMENT_WEIGHT_TRANSFER_REQUIRED')
            from .historical_binding import HistoryBindings
            self.history_bindings=HistoryBindings(history)
        expected_protocol=HISTORICAL_INFORMATION if history is not None else API_TARGETS if named else BULK_QUOTES if quotes else COMPONENT_DEBT if debt else ORDER_PAGES if pages else MEMORY_JOINT if joint else SCOPED_EXECUTION if scoped else QUERY_WINDOW if windows else CURRENT
        from .price_parameterization import validate as price_mode,ABSOLUTE
        if price_mode(config.get('price_parameterization',ABSOLUTE))!=ABSOLUTE and not config.get('full_depth_observations',False):
            raise ValueError('QUOTE_RANK_REQUIRES_DELIVERED_FULL_DEPTH')
        if protocol(config.get('observation_protocol',CURRENT))!=expected_protocol:
            raise ValueError('CURRENT_SIMULATOR_PROTOCOL_REQUIRED')
        from .resting_power import validate as funding_mode
        funding_mode(config.get('resting_buy_funding'))
        from .world_confidence import validate_lifetime
        lifetime=validate_lifetime(config.get('world_book_lifetime_ms'))
        required=self.original.manifest.get('requiredWorldBookLifetimeMs')
        if required is not None and lifetime!=required:raise ValueError('DATASET_REQUIRES_EXACT_WORLD_BOOK_LIFETIME')
        if lifetime is not None and(not self.development or config.get('snapshot_world_clock')is None):
            raise ValueError('WORLD_LIQUIDITY_CONFIDENCE_IS_EXPLICIT_SNAPSHOT_DEVELOPMENT_ONLY')
        # Exact source pins are part of the experiment identity, including
        # simulator, policy, reward, optimizer and role definitions.
        pins={str(p):hashlib.sha256(p.read_bytes()).hexdigest()
              for root in ('learning_actions','learning_realism','learning_v2','learning_flow')
              for p in (Path('moloch')/root).glob('*.py')if not p.name.startswith('test_')}
        self.frozen={'config':config,'dataset_manifest_sha256':self.dataset_sha,
            'realtime_refresh_mode':effective_refresh if config.get('sig_realtime_mode')is not None else None,
            'mechanics_sha256':hashlib.sha256(Path(config['mechanics']).read_bytes()).hexdigest(),
            'source_sha256':pins,'production_authority':False,
            'full_history_experiment':not self.development,'independent_test_used':False,
            'actual_split':{'competition_start_ms':self.split.competition_start_ms,'cutoff_ms':self.split.cutoff_ms},
            'actual_train_bounds_ms':list(self.train_bounds),'actual_evolution_bounds_ms':list(self.evolution_bounds),
            'training_reward_profile':{
                'mode':'human_complete_day_growth'if(not self.development or config.get('development_training_daily_growth',False))else 'short_development_log_wealth_proxy',
                'episode_ms':training_horizon(config,self.train_bounds,development=self.development),
                'development_only':self.development},
            'independent_test_source_claim':not self.development and independent_test_claim(self.original.manifest,self.split)}
        from .public_history_prefix import enabled,query_bounds,MODE
        self.public_history_prefix=enabled(config)
        self.frozen['public_trade_reporting_history']={
            'mode':MODE if self.public_history_prefix else 'legacy_same_scoring_role_only',
            'train_information_bounds_ms':list(self.train_bounds),
            'evolution_information_bounds_ms':list(query_bounds(self.train_bounds,self.evolution_bounds)
                if self.public_history_prefix else self.evolution_bounds),
            'test_information_bounds_ms':None if self.development else list(
                query_bounds(self.train_bounds,self.split.bounds('test')) if self.public_history_prefix
                else self.split.bounds('test')),
            'scoring_bounds_unchanged':True,'actor_access_requires_paid_read':True,
            'past_prints_generate_new_counterparty_fills':False}
        if not self.development:
            self.frozen['replay_specification_sha256']=hashlib.sha256(Path(config['specification']).read_bytes()).hexdigest()
        self.spec_sha=frozen(self.output/'experiment.json',self.frozen)
        if data_use is not None and not isinstance(data_use,DataUse):
            raise ValueError('ORIGINAL_DATA_USE_REGISTRY_REQUIRED')
        self.data_use=DataUse()if data_use is None else data_use
        # Register before collection, conservatively covering every training
        # and selection time the run may use, across all compiled versions.
        self.data_use.record('train',*self.train_bounds,self.spec_sha)
        selection_information=(query_bounds(self.train_bounds,self.evolution_bounds)
            if self.public_history_prefix else self.evolution_bounds)
        self.data_use.record('selection',*selection_information,self.spec_sha)
        self.population=[];self.calibration=None;self.total_decisions=0;self.total_updates=0
        self.last_snapshot_bucket=0
        self.tape_cache={}
        self.search_config=config.get('population_search')
        self.archive=None
        if self.search_config is not None:
            from .entropy_search import probability,coefficients
            probability(self.search_config.get('entropy_resample_probability',0))
            if 'entropy_coefficients'in self.search_config:coefficients(self.search_config['entropy_coefficients'])
            if type(self.search_config.get('retain_island_champions',True))is not bool:
                raise ValueError('EXPLICIT_RESEARCH_ELITISM_PROFILE')
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
        tape_type=ApiTape
        if self.config.get('historical_information')is not None:
            from .historical_neural import HistoricalNeuralTape
            tape_type=HistoricalNeuralTape
        elif self.config.get('api_target_affordances',False):
            from .api_affordances import ApiAffordanceTape
            tape_type=ApiAffordanceTape
        elif self.config.get('bulk_quote_observations',False):
            from .bulk_quotes import BulkQuoteTape
            tape_type=BulkQuoteTape
        elif self.config.get('component_debt_accounting',False):
            from .component_execution import ComponentTape
            tape_type=ComponentTape
        elif self.config.get('owned_order_pages',False):
            from .order_pages import OrderPageTape
            tape_type=OrderPageTape
        elif self.config.get('full_depth_observations',False):
            from .full_depth import FullDepthTape
            tape_type=FullDepthTape
        tape=tape_type.from_loaded(self.original,split=self.split,role=actual,
                                frozen_finalist_sha256=finalist)
        tape.maximum_boundary_gap_ms=self.gap
        tape.world_book_lifetime_ms=self.config.get('world_book_lifetime_ms')
        tape.global_state_encoding=self.config.get('global_state_encoding','raw')
        from .observation_protocol import CURRENT
        tape.observation_protocol=self.config.get('observation_protocol',CURRENT)
        tape.counterparty_replay_enabled=self.config.get('passive_flow_mode')is not None
        tape.counterparty_information_bounds=(self.train_bounds if role=='train'else
            self.evolution_bounds if role=='evolution'else self.split.bounds('test'))
        if getattr(self,'public_history_prefix',False):
            from .public_history_prefix import configure
            configure(tape,self.train_bounds,tape.counterparty_information_bounds)
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
        if getattr(self,'history_bindings',None)is not None:
            bounds=self.train_bounds if role=='train'else self.evolution_bounds if role=='evolution'else self.split.bounds('test')
            self.history_bindings.attach(env,role,bounds)
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
            env.realtime=DerivedStream(env,public_trades=self.config.get('derived_public_trade_stream',False),
                                      refresh_mode=self.config.get('sig_realtime_refresh_mode'))
            env.keys['SIG'].append(('realtime_token',))
            initial=self.config.get('initial_sig_realtime')
            if initial not in(None,'request_token'):raise ValueError('EXPLICIT_INITIAL_STREAM_STATE')
            if initial=='request_token':env.realtime.mint()
        if self.config.get('component_debt_accounting',False):
            from .component_execution import configure_from_evidence
            configure_from_evidence(env,self.config.get('component_evidence'))
        env.resting_buy_funding=self.config.get('resting_buy_funding')
        from .read_target_sampling import validate as sampling_mode,FLAT
        env.read_target_sampling=sampling_mode(self.config.get('read_target_sampling',FLAT))
        from .price_parameterization import validate as price_mode,ABSOLUTE
        env.price_parameterization=price_mode(self.config.get('price_parameterization',ABSOLUTE))
        from .historical_memory import profile as memory_profile
        env.historical_memory_profile=memory_profile(self.config.get('historical_memory_profile'))
        from .managed_data_service import specification,ManagedDataService
        service=specification(self.config.get('managed_data_service'))
        if service is not None:ManagedDataService(env,service)
        return env

    def stream(self,model,env,training=False):
        if self.calibration is None:
            self.calibration=calibrate(env,model)
            frozen(self.output/'compute-calibration.json',self.calibration)
        return Stream(env,model,calibration=self.calibration,record_training=training,
                      behavior_guard=self.config.get('on_policy_guard','sha256'),
                      actions_only=not training and self.config.get('evaluation_actions_only',False),
                      rollout_only=training and self.config.get('compact_rollout_statistics',False))

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
            logits=self.config.get('wait_prior_logits',[-3.,9.])
            if not isinstance(logits,list) or len(logits)!=2:
                raise ValueError('EXACT_WAIT_PRIOR_LOGIT_PAIR_REQUIRED')
            return initialize_wait_only(model,*logits)
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
        if self.config.get('closed_episode_return_replay',False)or self.config.get('training_worlds',1)>1:return self.train_parallel(model,optimizer)
        a,b=self.train_bounds
        length=training_horizon(self.config,self.train_bounds,development=self.development)
        start=self.rng.randrange(a,b-length+1);end=start+length
        env=self.world('train',start,end);stream=self.stream(model,env,True)
        ledger=training_ledger(env,self.config,development=self.development)
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
        count=self.config.get('training_worlds',1)
        minimum=1 if self.config.get('closed_episode_return_replay',False)else 2
        if type(count)is not int or count<minimum:raise ValueError('AT_LEAST_TWO_INDEPENDENT_TRAINING_WORLDS')
        a,b=self.train_bounds
        length=training_horizon(self.config,self.train_bounds,development=self.development)
        starts=[self.rng.randrange(a,b-length+1)for _ in range(count)]
        backend=None
        if self.config.get('process_source_worlds',False):
            # IPC is offline organization, not live actor compute cost. Measure
            # the original complete local actor before constructing any proxy.
            if self.calibration is None:
                from .episode_training import release_worlds
                local=self.world('train',starts[0],starts[0]+length)
                try:self.calibration=calibrate(local,model)
                finally:release_worlds([local])
                frozen(self.output/'compute-calibration.json',self.calibration)
            from .process_worlds import ProcessCohort,ExperimentFactory
            index=getattr(self,'source_cohort_counter',0);self.source_cohort_counter=index+1
            directory=self.output/'source-cohorts'/('cohort-%06d'%index)
            factories=[(ExperimentFactory(self.config,str(directory/('world-%03d'%i)),start,start+length),())
                       for i,start in enumerate(starts)]
            backend=ProcessCohort(factories);self._source_cohort=backend
            envs=backend.worlds;ledgers=[None]*len(envs)
            frozen(directory/'worker-identities.json',{'startMethod':'spawn',
                'workers':backend.identities,'worldBounds':[[e.now,e.end]for e in envs],
                'modelInParentOnly':True,'productionAuthority':False})
        else:
            envs=[self.world('train',start,start+length)for start in starts]
            ledgers=[training_ledger(e,self.config,development=self.development)for e in envs]
            if self.calibration is None:
                self.calibration=calibrate(envs[0],model)
                frozen(self.output/'compute-calibration.json',self.calibration)
        stream=BatchStream(envs,model,ledgers,calibration=self.calibration,
                           behavior_guard=self.config.get('on_policy_guard','sha256'),
                           compact_finished=self.config.get('compact_finished_worlds',False),
                           rollout_only=self.config.get('compact_rollout_statistics',False),world_backend=backend)
        closed=self.config.get('closed_episode_return_replay',False)
        if closed:self.train_closed_episode(stream,model,optimizer)
        while not closed and any(e.now<e.end for e in envs):
            chunk=stream.collect(self.config.get('compute_chunk_decisions',32))
            rewards=[f['economic_reward']for f in chunk.frames]
            reports=update(model,optimizer,chunk,rewards,role='train',epochs=self.config.get('ppo_epochs',1),**self.training_arguments(optimizer))
            self.total_updates+=sum(r['accepted']for r in reports)
            decisions=sum(int(f['learn'].sum())for f in chunk.frames)
            self.total_decisions+=decisions
            self.log({'event':'parallel_train_chunk','worlds':count,'decisions':decisions,
                'compute_batch_worlds':len(chunk.initial_hidden),
                'virtual_ms':[e.now for e in envs],'episode_end_ms':[e.end for e in envs],
                'completed_worlds':len(stream.completed),'updates':reports,
                'fills':sum(e.details['fill_count']if backend is not None else len(e.api_fills)for e in envs),
                'cuda_peak_bytes':torch.cuda.max_memory_allocated()if self.device.startswith('cuda')else 0})
            self.training_snapshot(model,optimizer)
        from .world_step import details
        summaries=[e.details if backend is not None else details(e)for e in envs]
        cases=[{**stream.completed[i],'family_counts':dict(stream.action_counts[i]),
                'passive_execution':summaries[i]['passive_execution'],
                'derived_stream':summaries[i]['derived_stream']}for i in range(count)]
        if backend is not None:backend.close();self._source_cohort=None
        return {'classification':'independent_simulated_worlds_not_additional_historical_evidence',
            'worlds':count,'reward':sum(c['reward']for c in cases)/count,
            'marked_roi':sum(c['marked_roi']for c in cases)/count,
            'fill_count':sum(c['fill_count']for c in cases),'eligible':False,
            'cases':cases,'all_worlds_preserve_financial_state_through_horizon':True}

    def train_closed_episode(self,stream,model,optimizer):
        from .episode_tape import EpisodeWriter,EpisodeReader
        from .episode_training import optimize
        index=getattr(self,'closed_episode_counter',0);self.closed_episode_counter=index+1
        path=self.output/'closed-episodes'/('cohort-%06d'%index)
        writer=EpisodeWriter(path,stream,dataset_sha256=self.dataset_sha,specification_sha256=self.spec_sha,
            observation_digest_mode=self.config.get('episode_observation_digest','legacy'))
        try:
            while any(e.now<e.end for e in stream.envs):
                chunk=stream.collect(self.config.get('compute_chunk_decisions',32),bootstrap=False);writer.record(chunk)
                decisions=sum(int(f['learn'].sum())for f in chunk.frames);self.total_decisions+=decisions
                self.log({'event':'closed_episode_rollout_chunk','decisions':decisions,'updates':[],
                    'behavior_weights_frozen':True,'virtual_ms':[e.now for e in stream.envs],
                    'completed_worlds':len(stream.completed)})
                del chunk
            manifest_sha=writer.seal()
        finally:writer.close()
        reader=EpisodeReader(path,manifest_sha)
        def ledger_factory(env):return training_ledger(env,self.config,development=self.development)
        def progress(rows,attempted):
            self.log({'event':'closed_episode_optimizer_progress','attempted_updates':attempted,
                'provisional_updates':rows,'cohort_commit_pending':True})
        try:
            reports=optimize(reader,model,optimizer,lambda a,b:self.world('train',a,b),ledger_factory,
                epochs=self.config.get('ppo_epochs',1),progress=progress,
                continue_after_kl_rejection=self.config.get('closed_episode_continue_after_kl_rejection',False),
                **self.training_arguments(optimizer))
            self.total_updates+=sum(r['accepted']for r in reports)
            self.log({'event':'parallel_train_chunk','phase':'closed_episode_optimization_committed',
                'decisions':0,'worlds':len(stream.envs),'updates':reports,'closed_episode_complete':True,
                'episode_manifest_sha256':manifest_sha,'full_episode_targets':True,
                'old_behavior_policy_frozen_during_collection':True,
                'cuda_peak_bytes':torch.cuda.max_memory_allocated()if self.device.startswith('cuda')else 0})
            self.training_snapshot(model,optimizer)
        finally:reader.close()

    def evaluate(self,model,checkpoint,role='evolution',finalist=None,*,confirmation=False):
        if role=='test'and self.development:raise ValueError('INSPECTED_DEVELOPMENT_HAS_NO_INDEPENDENT_TEST')
        if role=='test'and not independent_test_claim(self.original.manifest,self.split):
            raise ValueError('INDEPENDENT_TEST_SOURCE_PROVENANCE_NOT_ESTABLISHED')
        start,end=self.evolution_bounds if role=='evolution'else self.split.bounds('test')
        if role=='test':self.data_use.record('test',start,end,self.spec_sha)
        blocks=(self.search_config or{}).get('development_selection_blocks',1)
        from .development_calendars import calendars
        windows=calendars(start,end,blocks,development=self.development,
            episode_ms=self.config.get('development_selection_episode_ms'),confirmation=confirmation)
        descriptors=list((a,b,s)for a,b in windows for s in(False,True))
        batch_mode=self.config.get('batched_evaluation',False)
        if type(batch_mode)is not bool:raise ValueError('EXPLICIT_BATCHED_EVALUATION_PROFILE')
        batch_heads=self.config.get('batched_evaluation_action_heads',False)
        if type(batch_heads)is not bool or(batch_heads and not batch_mode):raise ValueError('BATCH_ACTION_HEADS_REQUIRE_BATCH_EVALUATION')
        if batch_mode:
            from .evaluation_batch import EvaluationBatch
            envs=[self.world(role,a,b,finalist,s)for a,b,s in descriptors]
            ledgers=[GrowthLedger(env,development=self.development,
                development_daily_growth=self.config.get('development_selection_daily_growth',False),
                historical_last_mark=self.config.get('development_last_observed_marks',False),
                maximum_mark_age_ms=self.config.get('maximum_mark_age_ms',120000))for env in envs]
            calibration=self.stream(model,envs[0]).calibration
            batch=EvaluationBatch(envs,model,[self.seed+90000+i for i in range(len(envs))],calibration=calibration,
                                  batch_action_heads=batch_heads,actions_only=self.config.get('evaluation_actions_only',False))
            while any(env.now<env.end for env in envs):
                batch.run_chunk(self.config.get('compute_chunk_decisions',32))
        cases=[]
        for seed_offset,(case_start,case_end,stress)in enumerate(descriptors):
            if batch_mode:
                from types import SimpleNamespace
                env=envs[seed_offset];ledger=ledgers[seed_offset]
                stream=SimpleNamespace(decisions=batch.decisions[seed_offset],action_counts=batch.action_counts[seed_offset],
                    status_counts=batch.status_counts[seed_offset],entity_counts=batch.entity_counts[seed_offset])
            else:
                seed_neural(self.seed+90000+seed_offset)
                env=self.world(role,case_start,case_end,finalist,stress);stream=self.stream(model,env)
                ledger=GrowthLedger(env,development=self.development,
                    development_daily_growth=self.config.get('development_selection_daily_growth',False),
                    historical_last_mark=self.config.get('development_last_observed_marks',False),
                    maximum_mark_age_ms=self.config.get('maximum_mark_age_ms',120000))
                while env.now<case_end:
                    stream.run_chunk(self.config.get('compute_chunk_decisions',32),deterministic=False)
                # Evaluation has no optimizer/dense reward labels. Require
                # start, actual daily boundaries and terminal wealth only.
                # A legitimate early passive fill may precede this role's
                # first recorded book; a compute chunk is not an economic
                # valuation boundary and must not reject that legal path.
            result=ledger.finish();cases.append({**result,'stress':stress,'start_ms':case_start,'end_ms':case_end,
                'selection_seed':self.seed+90000+seed_offset,'decisions':stream.decisions,
                'passive_execution':None if env.passive is None else {
                    'mode':env.passive.mode,'source_volume':env.passive.source_volume,
                    'intercepted_volume':env.passive.intercepted_volume,'calibrated':False},
                'family_counts':dict(stream.action_counts),'status_counts':dict(stream.status_counts),
                'entity_counts':dict(stream.entity_counts)})
            cases[-1]['valuation_observation_schedule']='start_daily_boundaries_and_terminal'
            cases[-1]['evaluation_compute_mode']='batch_encoding_case_local_rng'if batch_mode else'serial'
            if batch_heads:cases[-1]['evaluation_compute_mode']='batch_encoding_and_action_heads_case_local_generators'
        if role=='test'and self.data_use.overlaps(start,end,ignore_test=True):
            raise ValueError('TEST_CALENDAR_CONCURRENTLY_USED_FOR_OPTIMIZATION')
        return {'role':'development_selection'if self.development else role,
            'development_screen':self.development and self.config.get('development_selection_episode_ms')is not None and not confirmation,
            'full_window_development_confirmation':self.development and confirmation,
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
            from .entropy_search import offspring
            entropy,entropy_operator=offspring(donor_entropy,r['operator'],scales,self.rng,
                self.search_config.get('entropy_coefficients',
                    self.config.get('initial_entropy_coefficients',[self.config.get('entropy_coefficient',.01)])),
                resample_probability=self.search_config.get('entropy_resample_probability',0))
            opt=self.make_optimizer(model,rate,entropy)
            child=self.checkpoint('offspring-%03d-%03d'%(cycle,slot),model,opt)
            self.population[slot]=child
            self.log({'event':'population_replacement',**r,'mutation':report,'checkpoint':child,
                      'donor_checkpoint':donor,'probe_file_sha256':probe_file_sha,
                      'donor_source':donor_source,'parent_learning_rate':donor_rate,'child_learning_rate':rate,
                      'parent_entropy_coefficient':donor_entropy,'child_entropy_coefficient':entropy,
                      'entropy_mutation_operator':entropy_operator,
                      'child_receives_full_training_block_before_selection':True})
            del model,opt

    def policy_type(self):
        if self.config.get('historical_information')is not None:
            from .historical_neural import HistoricalNeuralPolicy
            return HistoricalNeuralPolicy
        if self.config.get('api_target_affordances',False):
            from .api_affordances import ApiAffordancePolicy
            return ApiAffordancePolicy
        if self.config.get('bulk_quote_observations',False):
            from .bulk_quotes import BulkQuotePolicy
            return BulkQuotePolicy
        if self.config.get('component_debt_accounting',False):
            from .component_execution import ComponentPolicy
            return ComponentPolicy
        if self.config.get('owned_order_pages',False):
            from .order_policy import OrderPagePolicy
            return OrderPagePolicy
        if self.config.get('joint_local_memory_density',False):
            from .joint_memory_policy import JointMemoryPolicy
            return JointMemoryPolicy
        if self.config.get('trade_query_windows',False):
            from .query_policy import QueryWindowPolicy
            return QueryWindowPolicy
        enabled=self.config.get('full_depth_observations',False)
        if type(enabled)is not bool:raise ValueError('EXPLICIT_FULL_DEPTH_OBSERVATION_PROFILE')
        if enabled:
            from .full_depth import FullDepthPolicy
            return FullDepthPolicy
        return ApiPolicy

    @neural_only
    def make_policy(self):
        model=self.policy_type()(self.model_size).to(self.device)
        from .acting_validation import validate as acting_mode,STRICT
        model.acting_validation=acting_mode(self.config.get('acting_validation',STRICT))
        model.global_state_encoding=self.config.get('global_state_encoding','raw')
        model.observation_protocol=self.config.get('observation_protocol',model.observation_protocol)
        from .read_target_sampling import validate as sampling_mode,FLAT
        model.read_target_sampling=sampling_mode(self.config.get('read_target_sampling',FLAT))
        from .price_parameterization import validate as price_mode,ABSOLUTE
        model.price_parameterization=price_mode(self.config.get('price_parameterization',ABSOLUTE))
        from .historical_memory import profile as memory_profile
        model.historical_memory_profile=memory_profile(self.config.get('historical_memory_profile'))
        from .managed_data_service import specification
        service=specification(self.config.get('managed_data_service'))
        model.managed_data_profile=None if service is None else service['profile']
        return model

    def founder_hyperparameters(self,member):
        entropies=self.config.get('initial_entropy_coefficients',[self.config.get('entropy_coefficient',.01)])
        rates=self.config.get('initial_learning_rates',[self.config.get('learning_rate',1e-5)])
        if not entropies or not rates:raise ValueError('NONEMPTY_FOUNDER_HYPERPARAMETERS')
        rate=rates[member%len(rates)];entropy=entropies[member%len(entropies)]
        if type(rate)not in(int,float)or not math.isfinite(rate)or rate<=0:raise ValueError('POSITIVE_FINITE_FOUNDER_LEARNING_RATE')
        if type(entropy)not in(int,float)or not math.isfinite(entropy)or entropy<0:raise ValueError('NONNEGATIVE_FINITE_FOUNDER_ENTROPY')
        return rate,entropy

    def member_order(self,cycle,members):
        order=self.config.get('first_cycle_member_order',list(range(members)))if cycle==0 else list(range(members))
        if not isinstance(order,list)or any(type(i)is not int for i in order)or sorted(order)!=list(range(members)):
            raise ValueError('FIRST_CYCLE_ORDER_MUST_INCLUDE_EVERY_MEMBER_ONCE')
        return order

    def run(self,*,cycle_callback=None):
        members=self.config.get('members',4);cycles=self.config.get('cycles',8)
        if type(members)is not int or members<2 or type(cycles)is not int or cycles<1:
            raise ValueError('POSITIVE_POPULATION_AND_CYCLE_COUNTS_REQUIRED')
        inherited=self.config.get('development_initial_checkpoint')
        parents=self.config.get('development_initial_checkpoints')or([inherited]if inherited else[])
        if parents and not self.development:raise ValueError('DEVELOPMENT_WEIGHT_TRANSFER_CANNOT_QUALIFY_FULL_HISTORY')
        fresh=self.config.get('fresh_founders',0)
        if type(fresh)is not int or not 0<=fresh<=members:raise ValueError('VALID_FRESH_LINEAGE_COUNT_REQUIRED')
        clock_shifts=self.config.get('inherited_wait_logit_shifts')
        if clock_shifts is not None:
            if (not self.development or not parents or not isinstance(clock_shifts,list)
                    or len(clock_shifts)!=members or any(x is not None for x in clock_shifts[members-fresh:])):
                raise ValueError('EXPLICIT_DEVELOPMENT_INHERITED_CLOCK_MUTATIONS_REQUIRED')
        family_shifts=self.config.get('founder_family_logit_shifts')
        if family_shifts is not None and(not isinstance(family_shifts,list)or len(family_shifts)!=members):
            raise ValueError('ONE_EXPLICIT_FAMILY_PRIOR_PER_FOUNDER_REQUIRED')
        price_shifts=self.config.get('founder_price_logit_shifts')
        if price_shifts is not None and(not isinstance(price_shifts,list)or len(price_shifts)!=members):
            raise ValueError('ONE_EXPLICIT_PRICE_PRIOR_PER_FOUNDER_REQUIRED')
        for member in range(members):
            with neural_section():
                seed_neural(self.seed+member)
                inherited=parents[member%len(parents)]if parents and member<members-fresh else None
                if inherited:
                    model,parent=load_checkpoint(inherited['path'],inherited['sha256'],self.device)
                    from .observation_protocol import migrate_development
                    migration=migrate_development(model,self.config)
                    if self.history_bindings is not None and type(model)is not self.policy_type():
                        raise ValueError('EXACT_HISTORICAL_ARCHITECTURE_TRANSFER_REQUIRED')
                    if migration is not None:
                        frozen(self.output/('founder-%03d-observation-protocol-migration.json'%member),migration)
                    target_encoding=self.config.get('global_state_encoding','raw')
                    if model.global_state_encoding!=target_encoding:
                        if self.config.get('migrate_global_state_encoding')is not True:
                            raise ValueError('EXPLICIT_GLOBAL_STATE_ENCODING_MIGRATION_REQUIRED')
                        frozen(self.output/('founder-%03d-state-encoding-migration.json'%member),{
                            'old':model.global_state_encoding,'new':target_encoding,'all_weights_retained':True,
                            'optimizer_moments_reset':True,'old_trajectories_discarded':True,'new_training_and_scores_required':True})
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
                    if type(model)is not self.policy_type():
                        from .api_affordances import ApiAffordancePolicy,migrate_api_affordances
                        if type(model)is ApiPolicy:
                            if self.config.get('migrate_full_depth_encoder')is not True or self.policy_type()is ApiPolicy:
                                raise ValueError('EXPLICIT_DEPTH_ARCHITECTURE_MIGRATION_REQUIRED')
                            from .full_depth import migrate_legacy_policy
                            model,migration=migrate_legacy_policy(model)
                            frozen(self.output/('founder-%03d-depth-migration.json'%member),migration)
                        from .full_depth import FullDepthPolicy
                        if type(model)is FullDepthPolicy and type(model)is not self.policy_type():
                            from .query_policy import QueryWindowPolicy,migrate_full_depth
                            from .joint_memory_policy import JointMemoryPolicy
                            from .order_policy import OrderPagePolicy
                            from .component_execution import ComponentPolicy
                            from .bulk_quotes import BulkQuotePolicy
                            if self.policy_type()not in(QueryWindowPolicy,JointMemoryPolicy,OrderPagePolicy,ComponentPolicy,BulkQuotePolicy,ApiAffordancePolicy)or self.config.get('migrate_query_window_heads')is not True:
                                raise ValueError('EXPLICIT_QUERY_WINDOW_ARCHITECTURE_MIGRATION_REQUIRED')
                            model,migration=migrate_full_depth(model)
                            frozen(self.output/('founder-%03d-query-window-migration.json'%member),migration)
                        from .query_policy import QueryWindowPolicy
                        if type(model)is QueryWindowPolicy and type(model)is not self.policy_type():
                            from .joint_memory_policy import JointMemoryPolicy,migrate_query_memory
                            from .order_policy import OrderPagePolicy
                            from .component_execution import ComponentPolicy
                            from .bulk_quotes import BulkQuotePolicy
                            if self.policy_type()not in(JointMemoryPolicy,OrderPagePolicy,ComponentPolicy,BulkQuotePolicy,ApiAffordancePolicy)or self.config.get('migrate_joint_memory_density')is not True:
                                raise ValueError('EXPLICIT_JOINT_MEMORY_ARCHITECTURE_MIGRATION_REQUIRED')
                            model,migration=migrate_query_memory(model)
                            frozen(self.output/('founder-%03d-joint-memory-migration.json'%member),migration)
                        from .joint_memory_policy import JointMemoryPolicy
                        if type(model)is JointMemoryPolicy and type(model)is not self.policy_type():
                            from .order_policy import OrderPagePolicy,migrate_owned_orders
                            from .component_execution import ComponentPolicy
                            from .bulk_quotes import BulkQuotePolicy
                            if self.policy_type()not in(OrderPagePolicy,ComponentPolicy,BulkQuotePolicy,ApiAffordancePolicy)or self.config.get('migrate_owned_order_pages')is not True:
                                raise ValueError('EXPLICIT_ORDER_PAGE_ARCHITECTURE_MIGRATION_REQUIRED')
                            model,migration=migrate_owned_orders(model)
                            frozen(self.output/('founder-%03d-order-page-migration.json'%member),migration)
                        from .order_policy import OrderPagePolicy
                        if type(model)is OrderPagePolicy and type(model)is not self.policy_type():
                            from .component_execution import ComponentPolicy,migrate_component_debt
                            from .bulk_quotes import BulkQuotePolicy
                            if self.policy_type()not in(ComponentPolicy,BulkQuotePolicy,ApiAffordancePolicy)or self.config.get('migrate_component_debt')is not True:
                                raise ValueError('EXPLICIT_COMPONENT_DEBT_MIGRATION_REQUIRED')
                            model,migration=migrate_component_debt(model)
                            frozen(self.output/('founder-%03d-component-debt-migration.json'%member),migration)
                        from .component_execution import ComponentPolicy
                        if type(model)is ComponentPolicy and type(model)is not self.policy_type():
                            from .bulk_quotes import BulkQuotePolicy,migrate_bulk_quotes
                            if self.policy_type()not in(BulkQuotePolicy,ApiAffordancePolicy)or self.config.get('migrate_bulk_quotes')is not True:
                                raise ValueError('EXPLICIT_BULK_QUOTE_MIGRATION_REQUIRED')
                            model,migration=migrate_bulk_quotes(model)
                            frozen(self.output/('founder-%03d-bulk-quote-migration.json'%member),migration)
                        if type(model)is not self.policy_type():
                            if self.policy_type()is not ApiAffordancePolicy or self.config.get('migrate_api_target_affordances')is not True:
                                raise ValueError('EXPLICIT_API_AFFORDANCE_MIGRATION_REQUIRED')
                            model,migration=migrate_api_affordances(model)
                            frozen(self.output/('founder-%03d-api-affordance-migration.json'%member),migration)
                    del parent
                    if member>=len(parents):
                        with torch.no_grad():
                            for p in model.parameters():p.add_(torch.randn_like(p)*self.config.get('mutation_std',1e-4))
                else:model=self.make_policy()
                model.global_state_encoding=self.config.get('global_state_encoding','raw')
                from .acting_validation import validate as acting_mode,STRICT
                target_mode=acting_mode(self.config.get('acting_validation',STRICT))
                if getattr(model,'acting_validation',STRICT)!=target_mode:
                    frozen(self.output/('founder-%03d-acting-validation-migration.json'%member),{
                        'old':getattr(model,'acting_validation',STRICT),'new':target_mode,
                        'weights_unchanged':True,'fresh_optimizer_and_trajectories':True})
                model.acting_validation=target_mode
                from .price_parameterization import validate as price_mode,ABSOLUTE
                old_price=price_mode(getattr(model,'price_parameterization',ABSOLUTE))
                from .historical_memory import bound_profile,profile as memory_profile
                old_memory=bound_profile(model);new_memory=memory_profile(self.config.get('historical_memory_profile'))
                if old_memory!=new_memory:
                    if not self.development or self.config.get('migrate_historical_memory_profile')is not True:
                        raise ValueError('EXPLICIT_DEVELOPMENT_HISTORICAL_MEMORY_MIGRATION_REQUIRED')
                    model.historical_memory_profile=new_memory
                    bound_profile(model)
                    frozen(self.output/('founder-%03d-historical-memory-migration.json'%member),{
                        'old':old_memory,'new':new_memory,'weightsPreserved':True,'optimizerMomentsReset':True,
                        'freshTrajectoriesAndScoresRequired':True})
                new_price=price_mode(self.config.get('price_parameterization',ABSOLUTE))
                if old_price!=new_price:
                    if self.config.get('migrate_price_parameterization')is not True:raise ValueError('EXPLICIT_PRICE_PARAMETERIZATION_MIGRATION_REQUIRED')
                    model.price_parameterization=new_price
                    frozen(self.output/('founder-%03d-price-parameterization-migration.json'%member),{
                        'old':old_price,'new':new_price,'weightsPreserved':True,'optimizerMomentsReset':True,
                        'freshTrajectoriesAndScoresRequired':True})
                from .managed_data_service import profile,specification
                service=specification(self.config.get('managed_data_service'))
                old_profile=profile(getattr(model,'managed_data_profile',None))
                new_profile=None if service is None else service['profile']
                if old_profile!=new_profile:
                    if self.config.get('migrate_managed_data_profile')is not True:
                        raise ValueError('EXPLICIT_MANAGED_DATA_PROFILE_MIGRATION_REQUIRED')
                    model.managed_data_profile=new_profile
                    frozen(self.output/('founder-%03d-managed-data-migration.json'%member),{
                        'old':old_profile,'new':new_profile,'weightsPreserved':True,
                        'optimizerMomentsReset':True,'freshTrajectoriesAndScoresRequired':True})
                from .read_target_sampling import validate as sampling_mode,FLAT
                old_sampling=sampling_mode(getattr(model,'read_target_sampling',FLAT))
                new_sampling=sampling_mode(self.config.get('read_target_sampling',FLAT))
                if old_sampling!=new_sampling:
                    if self.config.get('migrate_read_target_sampling')is not True:
                        raise ValueError('EXPLICIT_READ_SAMPLING_MIGRATION_REQUIRED')
                    model.read_target_sampling=new_sampling
                    frozen(self.output/('founder-%03d-read-sampling-migration.json'%member),{
                        'old':old_sampling,'new':new_sampling,'weightsPreserved':True,
                        'optimizerMomentsReset':True,'freshTrajectoriesAndScoresRequired':True})
                # Initialization changes exploration probability, never legal action
                # support: favor initially short alarms without forbidding long ones.
                if not inherited and self.config.get('initial_short_alarm_prior',True):
                    self.initialize_alarm_prior(model)
                if inherited and clock_shifts is not None and clock_shifts[member] is not None:
                    from .clock_prior import mutate_inherited_wait
                    frozen(self.output/('founder-%03d-inherited-clock-mutation.json'%member),
                           mutate_inherited_wait(model,clock_shifts[member]))
                if family_shifts is not None and family_shifts[member] is not None:
                    from .exploration_prior import mutate_family_logits
                    frozen(self.output/('founder-%03d-family-prior-mutation.json'%member),
                           mutate_family_logits(model,family_shifts[member]))
                rate,entropy=self.founder_hyperparameters(member)
                if price_shifts is not None and price_shifts[member]is not None:
                    from .exploration_prior import mutate_price_logits
                    frozen(self.output/('founder-%03d-price-prior-mutation.json'%member),mutate_price_logits(model,price_shifts[member]))
                opt=self.make_optimizer(model,rate=rate,entropy=entropy)
                self.population.append(self.checkpoint('founder-%03d'%member,model,opt))
                del model,opt
        best=None;island_champions={}
        for cycle in range(cycles):
            ranking=[]
            for member in self.member_order(cycle,members):
                previous=self.population[member]
                seed_neural(self.seed+cycle*1000+member)
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
            if self.search_config is not None and self.search_config.get('retain_island_champions',True):
                from .population import retain_island_champions
                ranking,island_champions,retentions=retain_island_champions(ranking,island_champions,
                    members=members,islands=self.search_config['islands'])
                for retained in retentions:
                    self.population[retained['slot']]=retained['retained_checkpoint']
                    self.log({'event':'research_champion_retained','cycle':cycle,**retained})
                frozen(self.output/('island-champions-%03d.json'%cycle),{
                    'classification':'fixed_search_incumbents_not_independent_evidence',
                    'dataset_sha256':self.dataset_sha,'experiment_sha256':self.spec_sha,
                    'champions':{str(k):v for k,v in island_champions.items()},
                    'independent_test_used':False,'production_authority':False})
            improved=best is None or ranking[0]['selection']['reward']>best['selection']['reward']
            if improved:best=ranking[0]
            # A short screen buys faster reproduction, never profitability
            # qualification. Recheck new positive screen winners on the full
            # development calendar; no holdout or gradient is used here.
            if (self.development and self.config.get('development_selection_episode_ms')is not None
                    and improved and best['selection']['reward']>0):
                cp=best['checkpoint'];model,bundle=load_checkpoint(cp['path'],cp['sha256'],self.device)
                del bundle
                confirmation=self.evaluate(model,cp,confirmation=True)
                frozen(self.output/('development-confirmation-%03d.json'%cycle),confirmation)
                self.log({'event':'development_full_window_confirmation','cycle':cycle,'checkpoint':cp,
                    'confirmation':confirmation,'screen_reward':best['selection']['reward']})
                del model
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
                seed_neural(self.seed+cycle+70000)
                with neural_section(),torch.no_grad():
                    for p in model.parameters():p.add_(torch.randn_like(p)*self.config.get('mutation_std',1e-4))
                opt=self.make_optimizer(model)
                self.population[loser]=self.checkpoint('offspring-%03d'%cycle,model,opt)
                del model,opt
            if cycle_callback is not None:cycle_callback(self,cycle,ranking,best)
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
    experiment=None
    try:
        experiment=Experiment(config,args.output);summary=experiment.run()
        print(json.dumps({'completed':True,'summary':str(args.output/'SUMMARY.json'),
                          'accepted_updates':summary['total_accepted_updates']}),flush=True)
    except Exception as exc:
        if args.output.exists():
            frozen(args.output/'FAILED.json',{'exception':type(exc).__name__,'reason':str(exc),
                'production_changed':False,'profitability_established':False})
        raise
    finally:
        if experiment is not None:experiment.close()


if __name__=='__main__':main()
