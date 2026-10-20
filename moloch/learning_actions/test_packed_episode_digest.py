"""Canonical actual-input byte hashes, not estimated source fingerprints."""
import tempfile,unittest
from pathlib import Path
from unittest.mock import patch
import torch
from .episode_tape import tensor_digest,packed_tensor_digest,observation_digest,EpisodeWriter,EpisodeReader


class PackedDigestTests(unittest.TestCase):
    def examples(self,device):
        bits=torch.tensor([0,-2147483648,2143289345,-4194303],dtype=torch.int32).view(torch.float32)
        return {'z':{'float_bits':bits.to(device),'bool':torch.tensor([[True,False]],device=device),
                     'integer':torch.tensor([-(2**63),2**63-1],device=device)},
                'a':torch.arange(24,dtype=torch.float64,device=device).reshape(2,3,4).transpose(0,2),
                'scalar':torch.tensor(-0.,device=device),'empty':torch.empty((2,0,3),device=device),
                'half':torch.tensor([1.,-2.,float('inf')],dtype=torch.float16,device=device)}

    def test_cpu_bits_headers_order_noncontiguous_scalar_and_empty_are_exact(self):
        for tree in({},self.examples('cpu'),{'empty':torch.empty(0)}):
            self.assertEqual(packed_tensor_digest(tree),tensor_digest(tree))

    @unittest.skipUnless(torch.cuda.is_available(),'CUDA required after active learner releases device')
    def test_cuda_actual_input_byte_hash_is_exact(self):
        tree=self.examples('cuda');self.assertEqual(packed_tensor_digest(tree),tensor_digest(tree))

    def test_fresh_tensor_mutation_cannot_reuse_an_old_digest(self):
        tree={'input':torch.zeros(3)};before=packed_tensor_digest(tree);tree['input'][1]=1
        self.assertNotEqual(before,packed_tensor_digest(tree))
        self.assertEqual(packed_tensor_digest(tree),tensor_digest(tree))

    def test_explicit_profile_and_legacy_default(self):
        self.assertIs(observation_digest('legacy'),tensor_digest)
        self.assertIs(observation_digest('packed_bytes_v1'),packed_tensor_digest)
        for mode in(None,True,'packed',[]):
            with self.assertRaises(ValueError):observation_digest(mode)

    def collect(self,path,*,corrupt=False):
        from . import test_episode_tape as fixture
        writer_type=EpisodeWriter
        def writer(*args,**kwargs):return writer_type(*args,**kwargs,observation_digest_mode='packed_bytes_v1')
        with patch.object(fixture,'EpisodeWriter',side_effect=writer):
            if corrupt:
                with patch('moloch.learning_actions.episode_tape.packed_tensor_digest',return_value='0'*64):
                    return fixture.EpisodeTapeTests().collect(path)
            return fixture.EpisodeTapeTests().collect(path)

    def test_packed_writer_reconstructs_under_independent_legacy_reader_and_trains(self):
        from .test_episode_tape import worlds
        from .episode_training import optimize
        from .economics import GrowthLedger
        torch.set_num_threads(1);torch.manual_seed(118)
        with tempfile.TemporaryDirectory()as directory:
            path=Path(directory)/'episode';writer,sha=self.collect(path);reader=EpisodeReader(path,sha)
            try:
                self.assertEqual(reader.metadata['observation_digest_mode'],'packed_bytes_v1')
                model=writer.stream.model;optimizer=torch.optim.Adam(model.parameters(),lr=1e-5)
                reports=optimize(reader,model,optimizer,lambda a,b:worlds()[[1101,1201,1301].index(b)],
                                 lambda e:GrowthLedger(e,development=True),continue_after_kl_rejection=True)
                self.assertEqual(len(reports),reader.metadata['batches']);self.assertTrue(any(r['accepted']for r in reports))
            finally:reader.close()

    def test_bad_packed_hash_is_still_rejected_by_legacy_source_reconstruction(self):
        from .test_episode_tape import worlds
        from .economics import GrowthLedger
        torch.set_num_threads(1);torch.manual_seed(118)
        with tempfile.TemporaryDirectory()as directory:
            path=Path(directory)/'episode';writer,sha=self.collect(path,corrupt=True);reader=EpisodeReader(path,sha)
            try:
                envs=worlds();ledgers=[GrowthLedger(e,development=True)for e in envs]
                with self.assertRaisesRegex(ValueError,'OBSERVATION_REQUIRED'):next(reader.replay(envs,ledgers))
            finally:reader.close()
