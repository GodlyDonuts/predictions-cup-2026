import copy,importlib.util,json,tempfile,unittest
from pathlib import Path
import numpy as np
from .replay_decoder import typed_equal,audit,audited_decoder,sha
from .test_disk_tape import dataset,build,book,prepared,episode,DiskFlowTape
from .contract import OrderSpec


class Registry:
    def __init__(self):self.rows=[]
    def record(self,*args):self.rows.append(args)


class TypedParityTests(unittest.TestCase):
    def test_exact_types_signed_zero_and_recursive_values(self):
        self.assertTrue(typed_equal({'a':[None,True,2,2.0,'x']},{'a':[None,True,2,2.0,'x']}))
        for a,b in [(True,1),(1,1.0),(-0.0,0.0),({'a':1},{'b':1}),([1],[1,2])]:
            self.assertFalse(typed_equal(a,b))
        self.assertIs(audited_decoder({},'none'),json.loads)
        with self.assertRaisesRegex(ValueError,'AUDIT_PIN'):audited_decoder({},'none','missing')


@unittest.skipUnless(importlib.util.find_spec('orjson'),'optional pinned decoder is not installed')
class ReplayDecoderTests(unittest.TestCase):
    def make(self,root,rows=None):
        source=dataset(root,rows=rows);cache=root/'cache';pin=build(source,cache);registry=Registry()
        result=audit(cache,pin,root/'audit',bounds=(0,6000),heldout_start=6000,data_use=registry)
        return source,cache,pin,result,registry

    def test_full_fixture_parity_is_pinned_to_cache_and_binary(self):
        with tempfile.TemporaryDirectory()as folder:
            root=Path(folder);source,cache,pin,result,registry=self.make(root)
            manifest=json.loads((cache/'manifest.json').read_bytes());proof=root/'audit/RESULT.json'
            decode=audited_decoder(manifest,pin,proof,sha(proof))
            self.assertEqual(decode('{"a":[1,2]}'),{'a':[1,2]})
            self.assertEqual(len(registry.rows),1)
            self.assertEqual(result['rows'],{t:manifest['counts'][t]for t in('market','flow')})
            for field,value in [('complete',False),('rows',{}),('implementation',{}),('cacheManifestSha256','changed')]:
                changed=copy.deepcopy(result);changed[field]=value;proof.write_text(json.dumps(changed))
                with self.assertRaisesRegex(ValueError,'PARITY_REQUIRED'):
                    audited_decoder(manifest,pin,proof,sha(proof))
            with self.assertRaisesRegex(ValueError,'AUDIT_CHANGED'):
                audited_decoder(manifest,pin,proof,'0'*64)

    def test_crossing_cache_is_rejected_before_decode_or_exposure(self):
        with tempfile.TemporaryDirectory()as folder:
            root=Path(folder);source=dataset(root);cache=root/'cache';pin=build(source,cache);registry=Registry()
            with self.assertRaisesRegex(ValueError,'PRECEDE_TEST'):
                audit(cache,pin,root/'audit',bounds=(0,4000),heldout_start=4000,data_use=registry)
            self.assertFalse(registry.rows);self.assertFalse((root/'audit').exists())

    def test_integer_precision_loss_rejects_entire_audit(self):
        with tempfile.TemporaryDirectory()as folder:
            root=Path(folder);row=book(0);row['precision_sentinel']=2**80+1
            source=dataset(root,rows=[row,book(5000)]);cache=root/'cache';pin=build(source,cache)
            with self.assertRaisesRegex(ValueError,'PARITY_FAILED'):
                audit(cache,pin,root/'audit',bounds=(0,6000),heldout_start=6000,data_use=Registry())
            self.assertFalse((root/'audit/RESULT.json').exists())

    def test_audited_reader_preserves_actor_and_economic_trajectory(self):
        with tempfile.TemporaryDirectory()as folder:
            root=Path(folder);source,cache,pin,_,_=self.make(root);proof=root/'audit/RESULT.json'
            left=DiskFlowTape(cache,pin,source)
            right=DiskFlowTape(cache,pin,source,decoder_audit=proof,decoder_audit_sha256=sha(proof))
            try:
                a,b=(episode(prepared(t))for t in(left,right))
                for env in(a,b):
                    env.request_observation('SIG',('book','x'));env.advance(201)
                    env.dispatch('ORDER',[OrderSpec('x','YES','BUY',4,450000)],idempotency_key='same')
                for at in(401,1500,2500,4000):
                    a.advance(at);b.advance(at);x,y=a.observe(),b.observe()
                    self.assertEqual(x['entities'],y['entities'])
                    for k in('contracts','global_state','present'):np.testing.assert_array_equal(x[k],y[k])
                    self.assertEqual((a.cash,a.lots,a.api_fills,a.realized,a.now),(b.cash,b.lots,b.api_fills,b.realized,b.now))
            finally:left.disk_store.close();right.disk_store.close()


if __name__=='__main__':unittest.main()
