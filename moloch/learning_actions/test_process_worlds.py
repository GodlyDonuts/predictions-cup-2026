"""Actual spawned-world/PPO parity, not throughput or profitability fixtures."""
import copy
from pathlib import Path
import tempfile
import unittest
import torch
from .process_worlds import ProcessCohort
from .test_episode_tape import worlds
from .economics import GrowthLedger
from .policy import ApiPolicy
from .batched import BatchStream
from .episode_tape import financial_digest,tensor_digest,observation_fields
from .training import update
from moloch.learning_v2.train_inner import fingerprint
from moloch.learning_realism.protocol import DAY


def factory(index,daily=False):
    torch.set_num_threads(1)
    env=worlds()[index]
    if daily:env.end=env.now+2*DAY
    ledger=GrowthLedger(env,development=True,development_daily_growth=daily,
                        historical_last_mark=True,maximum_mark_age_ms=3*DAY)
    return env,ledger,None


def wrong_role():
    env,ledger,_=factory(0);env.role='test';return env,ledger,None


class ProcessWorldTests(unittest.TestCase):
    def equal_tree(self,a,b):
        self.assertIs(type(a),type(b))
        if isinstance(a,torch.Tensor):self.assertTrue(torch.equal(a,b))
        elif isinstance(a,dict):
            self.assertEqual(a.keys(),b.keys())
            for key in a:self.equal_tree(a[key],b[key])
        elif isinstance(a,(tuple,list)):
            self.assertEqual(len(a),len(b))
            for x,y in zip(a,b):self.equal_tree(x,y)
        else:self.assertEqual(a,b)

    def run_case(self,remote,daily):
        torch.set_num_threads(1);torch.manual_seed(984)
        model=ApiPolicy(16);model.acting_validation='consolidated_v1'
        with torch.no_grad():
            model.heads['family'].weight.zero_();model.heads['family'].bias.zero_()
            model.heads['family'].bias[11]=100
        optimizer=torch.optim.Adam(model.parameters(),lr=1e-5)
        backend=None
        try:
            if remote:
                backend=ProcessCohort([(factory,(i,daily))for i in range(3)])
                self.assertTrue(all(not x['cudaInitialized']for x in backend.identities))
                envs=backend.worlds;ledgers=[None]*3
            else:
                pairs=[factory(i,daily)for i in range(3)]
                envs=[x[0]for x in pairs];ledgers=[x[1]for x in pairs]
            self.assertTrue(all((e.details['fill_count']if remote else len(e.api_fills))>0 for e in envs))
            stream=BatchStream(envs,model,ledgers,calibration={'decision_latency_ms':DAY//4 if daily else 100},
                behavior_guard='tensor_versions_v1',compact_finished=True,world_backend=backend)
            torch.manual_seed(773);frames=[];accepted=0;totals=[0.]*3
            while any(e.now<e.end for e in envs):
                chunk=stream.collect(3)
                for f in chunk.frames:
                    fields={k:f[k]for k in('actions','old_logprob','old_value','initial_hidden','reset','learn','terminal')}
                    frames.append((tensor_digest(observation_fields(f)),tensor_digest(fields),
                                   f['economic_reward'],f['elapsed_ms']))
                    # Compaction returns original world identities in its view.
                    indices=(chunk.stream.indices.tolist()if hasattr(chunk.stream,'indices')else list(range(3)))
                    for i,value in zip(indices,f['economic_reward']):totals[i]+=value
                accepted+=sum(r['accepted']for r in update(model,optimizer,chunk,
                    [f['economic_reward']for f in chunk.frames],role='train',epochs=1))
            self.assertGreater(accepted,0)
            self.assertTrue(all(r['fill_count']>=0 for r in stream.completed.values()))
            if daily:
                for i,r in stream.completed.items():
                    self.assertEqual(r['D'],2);self.assertAlmostEqual(totals[i],r['reward'],places=12)
            if remote:
                audits=[backend.request(e,'audit',None)for e in envs]
            else:
                from .stream import tensors
                audits=[{'financialSha256':financial_digest(e),
                         'observationSha256':tensor_digest(tensors(e.observe(),'cpu')),
                         'paidRequests':dict(e.feed_request_counts)}for e in envs]
            audits=[{k:a[k]for k in('financialSha256','observationSha256','paidRequests')}for a in audits]
            return (frames,fingerprint(model),copy.deepcopy(optimizer.state_dict()),stream.completed,audits,
                    stream.hidden.detach().clone(),torch.get_rng_state().clone(),stream.decisions)
        finally:
            if backend is not None:
                backend.close();self.assertTrue(all(not p.is_alive()for p,_ in backend.handles))

    def test_identical_actions_finance_rewards_hidden_states_and_optimizer(self):
        for daily in(False,True):
            with self.subTest(daily=daily):self.equal_tree(self.run_case(False,daily),self.run_case(True,daily))

    def test_remote_ipc_cannot_become_live_compute_latency(self):
        with self.assertRaisesRegex(ValueError,'IPC_MUST_NOT_CALIBRATE'):
            env,_,_=factory(0);BatchStream([env],ApiPolicy(16),[None],world_backend=object())

    def test_rejects_nontrain_child_and_does_not_restart_it(self):
        with self.assertRaisesRegex(RuntimeError,'MUST_BE_TRAIN_ROLE'):
            ProcessCohort([(wrong_role,())])

    def test_missing_worker_is_terminal_for_cohort_not_a_replacement_world(self):
        cohort=ProcessCohort([(factory,(0,))])
        try:
            process,_=cohort.handles[0];original=process.pid;process.terminate();process.join()
            with self.assertRaises((RuntimeError,OSError,EOFError)):
                cohort.prepare(cohort.worlds,[True])
            self.assertEqual(cohort.handles[0][0].pid,original)
            with self.assertRaisesRegex(ValueError,'CANNOT_RETRY'):
                cohort.prepare(cohort.worlds,[True])
        finally:cohort.close()

    def test_profile_validation_precedes_source_or_gpu_initialization(self):
        from .experiment import Experiment
        with tempfile.TemporaryDirectory()as td:
            for change in({'process_source_worlds':1},
                           {'process_source_worlds':True,'closed_episode_return_replay':True}):
                with self.assertRaises(ValueError):Experiment(change,Path(td)/'unused')
                self.assertFalse((Path(td)/'unused').exists())

    def test_full_experiment_entrypoint_uses_original_sqlite_and_identical_online_updates(self):
        from .experiment import Experiment
        from .test_disk_tape import dataset
        from .disk_tape import build
        from .snapshot_clock import MODE
        with tempfile.TemporaryDirectory()as td:
            root=Path(td);source=dataset(root);cache=root/'cache';digest=build(source,cache)
            mechanics=Path(__file__).resolve().parents[2]/'research/tartarus-learning-v2/development-mechanics-v2.json'
            config={'mode':'inspected_development','device':'cpu','cpu_threads':1,
                    'dataset':str(source),'mechanics':str(mechanics),
                    'replay_cache':{'path':str(cache),'manifest_sha256':digest},
                    'development_train_ms':[1,1001],'development_selection_ms':[1001,5000],
                    'development_episode_ms':200,'initial_world_history_ms':0,
                    'training_worlds':2,'compute_chunk_decisions':2,'snapshot_world_clock':MODE,
                    'on_policy_guard':'tensor_versions_v1','compact_finished_worlds':True}
            results=[]
            for remote in(False,True):
                experiment=Experiment(dict(config,process_source_worlds=remote),root/('run-'+str(remote)))
                try:
                    experiment.calibration={'decision_latency_ms':20}
                    model=ApiPolicy(16);optimizer=experiment.make_optimizer(model)
                    result=experiment.train_parallel(model,optimizer)
                    self.assertGreater(experiment.total_updates,0)
                    if remote:self.assertIsNone(experiment._source_cohort)
                    results.append((result,fingerprint(model),copy.deepcopy(optimizer.state_dict()),
                                    experiment.total_updates,experiment.total_decisions))
                finally:experiment.close()
            self.equal_tree(results[0],results[1])

    def test_spawn_admission_waits_on_real_registry_lock_but_keeps_holdout_rejection(self):
        from .process_worlds import WaitingDataUse
        from .selection import now_ms
        import threading,time
        with tempfile.TemporaryDirectory()as td:
            registry=WaitingDataUse(td);results=[]
            with registry._lock():
                thread=threading.Thread(target=lambda:results.append(registry.record('train',1,10,'a'*64)))
                thread.start();time.sleep(.08);self.assertTrue(thread.is_alive());self.assertEqual(results,[])
            thread.join(5);self.assertFalse(thread.is_alive());self.assertEqual(len(results),1)
            future=now_ms()+100000;registry.reserve_calendar(future,future+1000,'b'*64)
            with self.assertRaisesRegex(ValueError,'PROTECTED_HOLDOUT'):
                registry.record('train',future,future+1000,'c'*64)
