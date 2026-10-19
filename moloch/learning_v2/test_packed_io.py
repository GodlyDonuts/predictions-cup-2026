import pickle
import unittest
import torch
from .packed_io import ipc_pack,ipc_unpack


class IpcTests(unittest.TestCase):
    def test_transport_preserves_exact_types_shapes_noncontiguous_values(self):
        tree={'float':torch.tensor([[1.,float('inf')],[-0.,3.]]).t(),
              'masks':{'yes':torch.tensor([True,False]),'index':torch.tensor([2**50,-3])},
              'empty':torch.empty(0,3,dtype=torch.float64)}
        result=ipc_unpack(pickle.loads(pickle.dumps(ipc_pack(tree))))
        for expected,actual in [(tree['float'],result['float']),(tree['empty'],result['empty']),
                                (tree['masks']['yes'],result['masks']['yes']),(tree['masks']['index'],result['masks']['index'])]:
            self.assertEqual(expected.dtype,actual.dtype);self.assertEqual(expected.shape,actual.shape)
            self.assertTrue(torch.equal(expected,actual))

    def test_queued_payload_does_not_alias_mutable_producer_storage(self):
        original=torch.arange(10);payload=ipc_pack({'value':original[2:5]});original.fill_(99)
        restored=ipc_unpack(payload)['value'];self.assertEqual(restored.tolist(),[2,3,4])
        restored.fill_(7);self.assertEqual(original.tolist(),[99]*10)

    def test_non_tensor_or_non_numeric_payload_rejected(self):
        with self.assertRaises(TypeError):ipc_pack({'x':[1,2]})
        with self.assertRaises(TypeError):ipc_unpack({'x':'not array'})


if __name__=='__main__':unittest.main()
