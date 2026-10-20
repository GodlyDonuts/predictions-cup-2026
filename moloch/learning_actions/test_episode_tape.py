"""Synthetic closed-episode integrity/credit fixtures, not trading evidence."""
import tempfile,unittest
from pathlib import Path
import torch
from .episode_tape import EpisodeWriter,EpisodeReader
from .test_actions import environment
from .policy import ApiPolicy
from .contract import OrderSpec,UNIT
from .engine import ApiEpisode
from moloch.learning_v2.liquidity import Book
from .batched import BatchStream
from .economics import GrowthLedger
from .episode_training import optimize,EpisodeSession
from .training import update


def worlds():
    class FixtureEpisode(ApiEpisode):
        def _event(self,at,kind,value):
            if kind=='fixture_late_mark':
                self.books['x'].update(Book('x',at,at,((600000,100),),((650000,100),),'f'*64));return
            return super()._event(at,kind,value)
    result=[environment()for _ in range(3)]
    for i,e in enumerate(result):
        e.__class__=FixtureEpisode;e.clock.schedule(1050,10,'fixture_late_mark',None)
        e.end=1101+i*100;e._execute('ORDER',[OrderSpec('x','YES','BUY',1,450000)])
    result[1].scope['extra-fixture-market']=[]
    return result


class EpisodeTapeTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(118)

    def actor(self):
        model=ApiPolicy(16)
        with torch.no_grad():model.heads['family'].weight.zero_();model.heads['family'].bias.zero_();model.heads['family'].bias[11]=100
        return model

    def collect(self,path):
        envs=worlds();ledgers=[GrowthLedger(e,development=True)for e in envs]
        stream=BatchStream(envs,self.actor(),ledgers,calibration={'decision_latency_ms':100},
            behavior_guard='tensor_versions_v1',compact_finished=True)
        writer=EpisodeWriter(path,stream,dataset_sha256='a'*64,specification_sha256='b'*64)
        while any(e.now<e.end for e in envs):writer.record(stream.collect(3))
        return writer,writer.seal()

    def test_exact_reconstruction_compaction_and_full_episode_returns(self):
        with tempfile.TemporaryDirectory()as folder:
            path=Path(folder)/'episode';writer,sha=self.collect(path);reader=EpisodeReader(path,sha)
            try:
                envs=worlds();ledgers=[GrowthLedger(e,development=True)for e in envs];first=None;count=0
                for chunk,returns in reader.replay(envs,ledgers):
                    self.assertEqual(chunk.role,'train');count+=sum(int(f['learn'].sum())for f in chunk.frames)
                    if first is None:first=(chunk,returns)
                self.assertEqual(count,reader.metadata['decisions']);self.assertTrue(all(e.now==e.end for e in envs))
                self.assertTrue(all(l.closed for l in ledgers));chunk,returns=first
                self.assertAlmostEqual(returns[0][0],reader.metadata['results'][0]['reward'])
                self.assertNotEqual(returns[0][0],chunk.frames[0]['economic_reward'][0])
            finally:reader.close()

    def test_open_changed_policy_nontrain_and_changed_database_are_refused(self):
        with tempfile.TemporaryDirectory()as folder:
            root=Path(folder);envs=worlds();model=self.actor()
            stream=BatchStream(envs,model,[GrowthLedger(e,development=True)for e in envs],
                calibration={'decision_latency_ms':100},behavior_guard='tensor_versions_v1')
            writer=EpisodeWriter(root/'open',stream,dataset_sha256='a'*64,specification_sha256='b'*64)
            try:
                envs[0].role='evolution'
                with self.assertRaisesRegex(ValueError,'ONLY_TRAIN'):EpisodeWriter(root/'forbidden',stream,dataset_sha256='a'*64,specification_sha256='b'*64)
                envs[0].role='train'
                with self.assertRaisesRegex(ValueError,'MUST_COMPLETE'):writer.seal()
                chunk=stream.collect(1)
                with torch.no_grad():model.value.bias.add_(1)
                with self.assertRaisesRegex(ValueError,'POLICY_CHANGED'):writer.record(chunk)
            finally:writer.close()
            writer,sha=self.collect(root/'closed')
            with(root/'closed/episode.sqlite').open('ab')as f:f.write(b'changed fixture')
            with self.assertRaisesRegex(ValueError,'DATABASE_CHANGED'):EpisodeReader(root/'closed',sha)

    def test_changed_reconstruction_source_is_refused_before_training(self):
        with tempfile.TemporaryDirectory()as folder:
            path=Path(folder)/'episode';writer,sha=self.collect(path);reader=EpisodeReader(path,sha)
            try:
                envs=worlds();envs[0].known_cash+=UNIT;ledgers=[GrowthLedger(e,development=True)for e in envs]
                with self.assertRaisesRegex(ValueError,'OBSERVATION_REQUIRED'):next(reader.replay(envs,ledgers))
            finally:reader.close()

    def test_full_return_minibatches_update_critic_and_keep_original_behavior(self):
        with tempfile.TemporaryDirectory()as folder:
            path=Path(folder)/'episode';writer,sha=self.collect(path);reader=EpisodeReader(path,sha)
            try:
                model=writer.stream.model;before=model.value.weight.detach().clone();opt=torch.optim.Adam(model.parameters(),lr=1e-5)
                reports=optimize(reader,model,opt,lambda a,b:worlds()[[1101,1201,1301].index(b)],
                    lambda e:GrowthLedger(e,development=True),epochs=2)
                self.assertTrue(any(r['accepted']for r in reports));self.assertFalse(torch.equal(before,model.value.weight))
                self.assertTrue(all(r['return_source']=='sealed_full_training_episode'for r in reports))
                self.assertFalse(reader.metadata['independent_test_used'])
            finally:reader.close()

    def test_rejected_step_rolls_back_then_later_minibatches_can_train(self):
        from unittest.mock import patch
        from .episode_training import cpu_copy
        for continuation in(False,True):
            with self.subTest(continuation=continuation),tempfile.TemporaryDirectory()as folder:
                path=Path(folder)/'episode';writer,sha=self.collect(path);reader=EpisodeReader(path,sha)
                try:
                    model=writer.stream.model;before=cpu_copy(model.state_dict())
                    opt=torch.optim.Adam(model.parameters(),lr=1e-5);original_step=opt.step;calls=[]
                    def bad_first_step(*args,**kwargs):
                        result=original_step(*args,**kwargs);calls.append(True)
                        if len(calls)==1:
                            with torch.no_grad():
                                model.heads['family'].bias[11]=-100
                                model.heads['family'].bias[0]=100
                        return result
                    observations=[]
                    def progress(rows,n):
                        if n==1:
                            self.assertFalse(rows[0]['accepted']);self.assertGreater(rows[0]['post_update_kl'],.03)
                            self.assertEqual(opt.state_dict()['state'],{})
                            for name,value in model.state_dict().items():self.assertTrue(torch.equal(value,before[name]))
                        observations.extend(rows)
                    with patch.object(opt,'step',side_effect=bad_first_step):
                        reports=optimize(reader,model,opt,lambda a,b:worlds()[[1101,1201,1301].index(b)],
                            lambda e:GrowthLedger(e,development=True),continue_after_kl_rejection=continuation,progress=progress)
                    self.assertFalse(reports[0]['accepted'])
                    if continuation:
                        self.assertEqual(len(reports),reader.metadata['batches'])
                        self.assertTrue(any(r['accepted']for r in reports[1:]))
                        self.assertEqual([r['episode_batch']for r in reports],list(range(reader.metadata['batches'])))
                    else:self.assertEqual(len(reports),1)
                    self.assertEqual(len(observations),len(reports))
                finally:reader.close()

    def test_replay_failure_rolls_back_whole_learner_and_external_mutation_is_refused(self):
        from moloch.learning_v2.train_inner import fingerprint
        from .episode_training import cpu_copy
        with tempfile.TemporaryDirectory()as folder:
            path=Path(folder)/'episode';writer,sha=self.collect(path);reader=EpisodeReader(path,sha)
            try:
                model=writer.stream.model;original=fingerprint(model);opt=torch.optim.Adam(model.parameters(),lr=1e-5)
                # Populate Adam's state without changing behavior weights.
                for p in model.parameters():p.grad=torch.zeros_like(p)
                opt.step();opt.zero_grad(set_to_none=True);optimizer_before=cpu_copy(opt.state_dict())
                def wrong(a,b):
                    e=worlds()[[1101,1201,1301].index(b)];base=type(e)
                    def event(self,at,kind,value):
                        if kind=='fixture_late_mismatch':self.known_cash+=UNIT;return
                        return base._event(self,at,kind,value)
                    e.__class__=type('WrongFixtureEpisode',(base,),{'_event':event})
                    e.clock.schedule(550,10,'fixture_late_mismatch',None);return e
                attempted=[]
                with self.assertRaisesRegex(ValueError,'OBSERVATION_REQUIRED'):
                    optimize(reader,model,opt,wrong,lambda e:GrowthLedger(e,development=True),progress=lambda rows,n:attempted.extend(rows))
                self.assertTrue(any(r['accepted']for r in attempted))
                self.assertEqual(fingerprint(model),original)
                optimizer_after=cpu_copy(opt.state_dict())
                self.assertEqual(optimizer_before['param_groups'],optimizer_after['param_groups'])
                for key,values in optimizer_before['state'].items():
                    for name,value in values.items():self.assertTrue(torch.equal(value,optimizer_after['state'][key][name]))
                session=EpisodeSession(reader,model);envs=worlds();ledgers=[GrowthLedger(e,development=True)for e in envs]
                chunk,_=next(reader.replay(envs,ledgers))
                with torch.no_grad():model.value.bias.add_(1)
                with self.assertRaisesRegex(ValueError,'STUDENT_CHANGED'):
                    update(model,opt,chunk,[f['economic_reward']for f in chunk.frames],role='train',episode_session=session)
            finally:reader.close()

    def test_experiment_connects_closed_rollouts_optimization_selection_and_evolution(self):
        from .test_population_experiment import FixtureExperiment
        with tempfile.TemporaryDirectory()as folder:
            exp=FixtureExperiment(Path(folder));exp.config.update(closed_episode_return_replay=True,
                closed_episode_continue_after_kl_rejection=True,members=2,fresh_founders=2)
            exp.search_config['islands']=1;result=exp.run()
            self.assertGreater(result['total_accepted_updates'],0)
            closed=[r for r in exp.events if r.get('phase')=='closed_episode_optimization_committed']
            self.assertEqual(len(closed),4);self.assertTrue(all(r['full_episode_targets']for r in closed))
            self.assertTrue(all(u['continue_after_kl_rejection']for r in closed for u in r['updates']))
            self.assertEqual(len([r for r in exp.events if r['event']=='population_replacement']),1)
            self.assertFalse(result['independent_test_used']);self.assertFalse(result['production_changed'])
