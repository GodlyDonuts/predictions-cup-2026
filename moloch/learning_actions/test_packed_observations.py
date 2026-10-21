import unittest
import numpy as np
import torch
import torch.nn.functional as F
from .stream import tensors
from .packed_observations import assemble
from .batched import inputs
from moloch.learning_v2.packed_io import transfer,_whole_cpu_allocation


def legacy(observations,device='cpu'):
    values=[tensors(o,'cpu')for o in observations]
    n=max(v['contracts'].shape[1]for v in values)
    out={'contracts':torch.cat([F.pad(v['contracts'],(0,0,0,n-v['contracts'].shape[1]))for v in values]),
        'global_state':torch.cat([v['global_state']for v in values]),
        'present':torch.cat([F.pad(v['present'],(0,n-v['contracts'].shape[1]))for v in values]),
        'masks':{k:torch.cat([F.pad(v['masks'][k],(0,n-v['contracts'].shape[1])if k=='market'
                    else(0,0,0,n-v['contracts'].shape[1]))for v in values])for k in values[0]['masks']}}
    return transfer(out,device)if torch.device(device).type!='cpu'else out


def observation(n,seed=1,features=517):
    rng=np.random.default_rng(seed)
    return {'contracts':rng.standard_normal((n,features)).astype(np.float32),
            'global_state':rng.standard_normal(11).astype(np.float32),
            'present':rng.integers(0,2,n).astype(bool),
            'masks':{'market':rng.integers(0,2,n).astype(bool),
                     'family':rng.integers(0,2,(n,12)).astype(bool),
                     'trade_query':rng.integers(0,2,(n,1)).astype(bool)}}


class PackedObservationTests(unittest.TestCase):
    def equal(self,a,b):
        self.assertEqual(tuple(a),tuple(b))
        for k in a:
            if isinstance(a[k],dict):self.equal(a[k],b[k]);continue
            self.assertEqual(a[k].dtype,b[k].dtype);self.assertEqual(a[k].shape,b[k].shape)
            # Preserve signed zeros and NaN bit patterns as well as values.
            self.assertEqual(a[k].detach().cpu().contiguous().numpy().tobytes(),b[k].detach().cpu().contiguous().numpy().tobytes())

    def test_ragged_empty_single_and_large_batches_match_original_bytes(self):
        for lengths in [(1,),(0,3),(5,1,9,0),(25,125,400,89)]:
            rows=[observation(n,i)for i,n in enumerate(lengths)]
            rows[0]['contracts'].flat[:min(rows[0]['contracts'].size,3)]=[-0.,np.inf,np.nan][:min(rows[0]['contracts'].size,3)]
            self.equal(legacy(rows),inputs(rows,'cpu'))
            self.equal(legacy(rows),assemble(rows))

    def test_noncontiguous_values_and_promoted_types(self):
        rows=[observation(4),observation(2)]
        rows[0]['contracts']=rows[0]['contracts'][:,::2]
        rows[1]['contracts']=rows[1]['contracts'][:,::2].astype(np.float64)
        rows[0]['global_state']=rows[0]['global_state'].astype(np.float64)
        self.equal(legacy(rows),assemble(rows))

    def test_outputs_own_source_bytes_and_disjoint_leaf_ranges(self):
        rows=[observation(3),observation(1)];actual=assemble(rows);expected=legacy(rows)
        rows[0]['contracts'].fill(900);rows[0]['masks']['family'].fill(False)
        self.equal(expected,actual)
        actual['contracts'].fill_(700)
        self.equal({'global_state':expected['global_state'],'masks':expected['masks']},
                   {'global_state':actual['global_state'],'masks':actual['masks']})

    def test_fast_upload_requires_complete_contiguous_original_allocation(self):
        x=assemble([observation(5),observation(2)])
        for values in [[x['contracts'].reshape(-1),x['global_state'].reshape(-1)],
                       [x['present'].reshape(-1),*[v.reshape(-1)for v in x['masks'].values()]]]:
            packed=_whole_cpu_allocation(values);self.assertIsNotNone(packed)
            self.assertTrue(torch.equal(packed,torch.cat(values)))
            self.assertIsNone(_whole_cpu_allocation(list(reversed(values))))
            self.assertIsNone(_whole_cpu_allocation(values[:-1]))
        whole=torch.arange(12);self.assertIsNone(_whole_cpu_allocation([whole[2:5],whole[5:]]))
        self.assertIsNone(_whole_cpu_allocation([torch.arange(2),torch.arange(3)]))

    @unittest.skipUnless(torch.cuda.is_available(),'CUDA unavailable')
    def test_cuda_upload_and_source_mutation_match_original(self):
        for lengths in [(47,2,99),(847,1020,1500)]:
            rows=[observation(n,i)for i,n in enumerate(lengths)]
            actual=inputs(rows,'cuda');expected=legacy(rows,'cuda');self.equal(expected,actual)
            rows[0]['contracts'].fill(0);self.equal(expected,actual)

    @unittest.skipUnless(torch.cuda.is_available(),'CUDA unavailable')
    def test_actual_historical_policy_outputs_and_gradients_match(self):
        from .historical_neural import HistoricalNeuralPolicy
        rows=[observation(n,i)for i,n in enumerate((1100,800,1050))]
        for row in rows:
            row['present'].fill(True)
            for mask in row['masks'].values():mask.fill(True)
        # Use the actual architecture's global-state width, not synthetic
        # padding interpreted as a changed observation specification.
        from .test_actions import environment
        width=environment().observe()['global_state'].shape[0]
        for row in rows:row['global_state']=np.zeros(width,np.float32)
        torch.manual_seed(6291);a=HistoricalNeuralPolicy(32).cuda()
        b=HistoricalNeuralPolicy(32).cuda();b.load_state_dict(a.state_dict())
        results=[]
        for model,prepared in [(a,legacy(rows,'cuda')),(b,inputs(rows,'cuda'))]:
            torch.manual_seed(912)
            out=model.step(**prepared,hidden=model.initial_state(len(rows)),
                           reset=torch.ones(len(rows),device='cuda',dtype=torch.bool),deterministic=True)
            loss=out['value'].square().mean()+out['logprob'].mean()
            self.assertTrue(torch.isfinite(loss));loss.backward()
            results.append(out)
        for name in ('value','logprob','hidden'):self.assertTrue(torch.equal(results[0][name],results[1][name]),name)
        self.equal(results[0]['actions'],results[1]['actions'])
        gradients=0
        for (name,p),(other,q)in zip(a.named_parameters(),b.named_parameters()):
            self.assertEqual(name,other);self.assertEqual(p.grad is None,q.grad is None)
            if p.grad is not None:
                gradients+=1;self.assertTrue(torch.equal(p.grad,q.grad),name)
        self.assertGreater(gradients,0)


if __name__=='__main__':unittest.main()
