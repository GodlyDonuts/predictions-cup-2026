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
