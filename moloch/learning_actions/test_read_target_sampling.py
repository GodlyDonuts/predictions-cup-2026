import tempfile,types,unittest
from pathlib import Path
import torch
from .read_target_sampling import FLAT,BALANCED,groups,logits
from .behavior_guard import contract,require_contract
from .test_actions import environment
from .test_order_pages import world
from .order_policy import OrderPagePolicy
from .stream import tensors
from .training import save_checkpoint,load_checkpoint,sequence_forward
from .state_encoding import matching


class ReadSamplingTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(31)

    def inputs(self):
        entities=[('read',('Kalshi',('historical_trades',str(i))))for i in range(100)]
        entities += [('read',('SIG',('realtime_token',))),('read',('SIG',('book','x')))]
        return torch.zeros(1,102),{'read_groups':torch.as_tensor(groups(entities)).unsqueeze(0)}

    def test_equal_endpoint_mass_does_not_depend_on_catalog_size(self):
        raw,masks=self.inputs();model=types.SimpleNamespace(read_target_sampling=BALANCED)
        adjusted=logits(model,raw,torch.tensor([10]),torch.ones_like(raw,dtype=torch.bool),masks)
        p=adjusted.softmax(-1)
        self.assertAlmostEqual(float(p[0,:100].sum()),1/3,places=6)
        self.assertAlmostEqual(float(p[0,100]),1/3,places=6)
        self.assertTrue(bool((p>0).all()))
        self.assertTrue(torch.equal(logits(model,raw,torch.tensor([1]),torch.ones_like(raw,dtype=torch.bool),masks),raw))

    def test_only_currently_eligible_targets_count_and_gradients_survive(self):
        raw,masks=self.inputs();raw.requires_grad_(True);eligible=torch.ones_like(raw,dtype=torch.bool);eligible[0,:99]=False
        y=logits(types.SimpleNamespace(read_target_sampling=BALANCED),raw,torch.tensor([10]),eligible,masks)
        p=y.masked_fill(~eligible,-torch.inf).softmax(-1)
        self.assertAlmostEqual(float(p[0,99].detach()),1/3,places=6)
        (-p[0,100].log()).backward();self.assertTrue(bool(torch.isfinite(raw.grad).all()))
        self.assertNotEqual(float(raw.grad[0,100]),0.)

    def test_legacy_is_identical_and_missing_metadata_fails(self):
        raw,masks=self.inputs();model=types.SimpleNamespace();eligible=torch.ones_like(raw,dtype=torch.bool)
        self.assertIs(logits(model,raw,torch.tensor([10]),eligible,{}),raw)
        model.read_target_sampling=BALANCED
        with self.assertRaisesRegex(ValueError,'GROUP_METADATA'):logits(model,raw,torch.tensor([10]),eligible,{})
        model.read_target_sampling='anything'
        with self.assertRaisesRegex(ValueError,'SAMPLING_LAW'):logits(model,raw,torch.tensor([10]),eligible,masks)

    def test_observation_keeps_every_original_mechanics_mask(self):
        e=environment();before=e.observe();e.read_target_sampling=BALANCED;after=e.observe()
        self.assertEqual(before['entities'],after['entities'])
        for name,value in before['masks'].items():self.assertTrue((value==after['masks'][name]).all(),name)
        self.assertEqual(set(after['masks'])-set(before['masks']),{'read_groups'})
        self.assertTrue((after['contracts']==before['contracts']).all())
        e.read_target_sampling=FLAT;self.assertNotIn('read_groups',e.observe()['masks'])

    def test_checkpoint_and_on_policy_contract_pin_sampling_law(self):
        m=OrderPagePolicy(16);legacy=contract(m);self.assertEqual(len(legacy),5)
        m.read_target_sampling=BALANCED
        with self.assertRaisesRegex(ValueError,'EXPLICIT_BEHAVIOR'):require_contract(m,None)
        with self.assertRaisesRegex(ValueError,'PROTOCOL_CHANGED'):require_contract(m,legacy)
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'model.pt';opt=torch.optim.Adam(m.parameters());digest=save_checkpoint(p,m,opt,dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            loaded,bundle=load_checkpoint(p,digest)
            self.assertEqual(loaded.read_target_sampling,BALANCED);self.assertEqual(contract(loaded),contract(m))
            self.assertEqual(bundle['read_target_sampling'],BALANCED)

    def test_full_rollout_and_actions_only_share_the_exact_sampling_law(self):
        e=world();e.read_target_sampling=BALANCED;m=OrderPagePolicy(16);m.read_target_sampling=BALANCED
        x=tensors(e.observe(),'cpu');h=m.initial_state(1);reset=torch.ones(1,dtype=torch.bool)
        outputs=[]
        for fn in(m.step,m.step_rollout,m.step_actions_only):
            torch.manual_seed(77)
            with torch.no_grad():outputs.append(fn(**x,hidden=h,reset=reset))
        for row in outputs[1:]:
            for k,v in outputs[0]['actions'].items():self.assertTrue(torch.equal(row['actions'][k],v),k)
        self.assertTrue(torch.equal(outputs[0]['logprob'],outputs[1]['logprob']))
        action={k:v.clone()for k,v in outputs[0]['actions'].items()};action['family'][:]=10
        read=x['masks']['family'][0,:,10].nonzero().flatten()[0];action['market'][:]=read
        forced=m.step(**x,hidden=h,reset=reset,actions=action)
        frame={**x,'actions':action,'reset':reset}
        replay,_=sequence_forward(m,[frame],h)
        self.assertTrue(torch.allclose(replay['logprob'][0],forced['logprob'],atol=1e-10,rtol=1e-10))
        m.read_target_sampling=FLAT
        with self.assertRaisesRegex(ValueError,'READ_SAMPLING_MISMATCH'):matching(e,m)


if __name__=='__main__':unittest.main()
