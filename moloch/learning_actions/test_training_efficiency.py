import unittest
import torch
from .behavior_guard import identity
from .policy import ApiPolicy
from .stream import Stream
from .training import collect_chunk,sequence_forward,update
from .test_actions import environment


class TrainingEfficiencyTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(731)

    def chunk(self,model,guard='sha256'):
        with torch.no_grad():model.fractions.bias[2]=-10.;model.fractions.bias[3]=100.
        stream=Stream(environment(),model,calibration={'decision_latency_ms':1},record_training=True,behavior_guard=guard)
        return collect_chunk(stream,16)

    def test_checkpointed_encoder_preserves_values_and_parameter_gradients(self):
        model=ApiPolicy(16);chunk=self.chunk(model);outputs=[];gradients=[]
        for enabled in (False,True):
            model.zero_grad(set_to_none=True)
            out,_=sequence_forward(model,chunk.frames,chunk.initial_hidden,checkpoint_encoder=enabled)
            (out['logprob']+out['value']+.05*out['entropy']).sum().backward()
            outputs.append({k:v.detach().clone()for k,v in out.items()})
            gradients.append({n:p.grad.detach().clone()for n,p in model.named_parameters()if p.grad is not None})
        for k in outputs[0]:torch.testing.assert_close(outputs[0][k],outputs[1][k],atol=0,rtol=0)
        for k in gradients[0]:torch.testing.assert_close(gradients[0][k],gradients[1][k],atol=0,rtol=0)

    def test_fast_guard_rejects_supported_mutations_and_accepts_unchanged_chunks(self):
        for mutation in ('in_place','replacement','load_state','optimizer_step'):
            model=ApiPolicy(16);chunk=self.chunk(model,'tensor_versions_v1');before=identity(model,'tensor_versions_v1')
            if mutation=='in_place':
                with torch.no_grad():model.value.bias.add_(.001)
            elif mutation=='replacement':model.value.bias=torch.nn.Parameter(model.value.bias.detach().clone())
            elif mutation=='load_state':model.load_state_dict(model.state_dict())
            else:
                optimizer=torch.optim.Adam(model.parameters(),lr=1e-4)
                sum(p.sum()for p in model.parameters()).backward();optimizer.step()
            self.assertNotEqual(before,identity(model,'tensor_versions_v1'))
            with self.assertRaisesRegex(ValueError,'FRESH_ON_POLICY'):
                update(model,torch.optim.Adam(model.parameters()),chunk,[0.]*len(chunk.frames),role='train')
        model=ApiPolicy(16);chunk=self.chunk(model,'tensor_versions_v1')
        reports=update(model,torch.optim.Adam(model.parameters(),lr=1e-5),chunk,[0.]*len(chunk.frames),
                       role='train',epochs=1,checkpoint_encoder=True,entropy_coefficient=0.)
        self.assertEqual(reports[0]['behavior_guard'],'tensor_versions_v1')
        self.assertEqual(reports[0]['entropy_coefficient'],0.)
        self.assertIn('policy_loss',reports[0]);self.assertIn('mean_entropy',reports[0])

    def test_guard_does_not_accept_other_model_instances_or_unknown_modes(self):
        a=ApiPolicy(16);b=ApiPolicy(16);b.load_state_dict(a.state_dict())
        self.assertNotEqual(identity(a,'tensor_versions_v1'),identity(b,'tensor_versions_v1'))
        with self.assertRaisesRegex(ValueError,'EXPLICIT_ON_POLICY'):identity(a,'skip')

    def compact_stream(self,model,enabled,*,staggered=False):
        from .batched import BatchStream
        from .economics import GrowthLedger
        envs=[environment()for _ in range(4)]
        ledgers=[GrowthLedger(e,development=True)for e in envs]
        for i in (1,3):envs[i].advance(envs[i].end)
        if staggered:envs[0].end=envs[0].now+2;envs[2].end=envs[2].now+8
        stream=BatchStream(envs,model,ledgers,calibration={'decision_latency_ms':1},
                           behavior_guard='tensor_versions_v1',compact_finished=enabled)
        with torch.no_grad():
            for head in model.heads.values():head.weight.zero_();head.bias.zero_()
            model.heads['family'].bias[11]=30.
        stream.hidden=torch.randn_like(stream.hidden)
        return stream

    def test_compact_noncontiguous_worlds_match_full_live_trajectories_and_keep_global_ids(self):
        a=ApiPolicy(16);b=ApiPolicy(16);b.load_state_dict(a.state_dict())
        full=self.compact_stream(a,False);compact=self.compact_stream(b,True)
        compact.hidden=full.hidden.clone();before=compact.hidden.clone()
        x=full.collect(4,deterministic=True);y=compact.collect(4,deterministic=True)
        self.assertEqual(tuple(y.initial_hidden.shape),(2,16))
        for j,i in enumerate((0,2)):
            self.assertEqual(full.envs[i].now,compact.envs[i].now)
            self.assertEqual(full.envs[i].cash,compact.envs[i].cash)
            self.assertEqual(full.envs[i].api_fills,compact.envs[i].api_fills)
            torch.testing.assert_close(full.hidden[i],compact.hidden[i],atol=1e-6,rtol=1e-5)
            for f,g in zip(x.frames,y.frames):
                for key in f['actions']:torch.testing.assert_close(f['actions'][key][i],g['actions'][key][j])
                torch.testing.assert_close(f['old_logprob'][i],g['old_logprob'][j],atol=1e-6,rtol=1e-5)
        torch.testing.assert_close(before[[1,3]],compact.hidden[[1,3]],atol=0,rtol=0)
        self.assertEqual(compact.decisions,[4,0,4,0])
        self.assertEqual(sum(compact.action_counts[2].values()),4)

    def test_accepted_update_writes_hidden_back_and_does_not_mutate_chunk_initial_memory(self):
        model=ApiPolicy(16);stream=self.compact_stream(model,True)
        before=stream.hidden.clone();chunk=stream.collect(4,deterministic=True)
        initial=chunk.initial_hidden.clone()
        reports=update(model,torch.optim.Adam(model.parameters(),lr=1e-6),chunk,
                       [f['economic_reward']for f in chunk.frames],role='train',epochs=1)
        self.assertTrue(reports[0]['accepted'])
        _,expected=sequence_forward(model,chunk.frames,chunk.initial_hidden)
        torch.testing.assert_close(stream.hidden[[0,2]],expected)
        torch.testing.assert_close(stream.hidden[[1,3]],before[[1,3]],atol=0,rtol=0)
        torch.testing.assert_close(chunk.initial_hidden,initial,atol=0,rtol=0)
        next_chunk=stream.collect(2,deterministic=True)
        torch.testing.assert_close(next_chunk.initial_hidden,expected)

    def test_staggered_terminal_worlds_compact_only_at_chunk_boundary_and_keep_results(self):
        stream=self.compact_stream(ApiPolicy(16),True,staggered=True)
        first=stream.collect(4,deterministic=True)
        self.assertEqual(tuple(first.initial_hidden.shape),(2,16))
        self.assertEqual(first.frames[-1]['learn'].tolist(),[False,True])
        self.assertIn(0,stream.completed);self.assertNotIn(2,stream.completed)
        frozen=stream.hidden[0].clone();first_result=stream.completed[0]
        second=stream.collect(4,deterministic=True)
        self.assertEqual(tuple(second.initial_hidden.shape),(1,16))
        self.assertIn(2,stream.completed);self.assertEqual(stream.completed[0],first_result)
        torch.testing.assert_close(stream.hidden[0],frozen,atol=0,rtol=0)
        self.assertEqual(stream.decisions,[2,0,8,0])
        with self.assertRaisesRegex(ValueError,'NO_ACTIVE_WORLD'):stream.collect(1)

    def test_finished_world_compaction_configuration_must_be_explicit_boolean(self):
        with self.assertRaisesRegex(ValueError,'EXPLICIT_FINISHED_WORLD'):self.compact_stream(ApiPolicy(16),1)
