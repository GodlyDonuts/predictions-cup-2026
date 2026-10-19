import base64
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from .capture_native_stream import NativeVerifier
from .native_capture_tail import anchor


def h(raw):return hashlib.sha256(raw).hexdigest()
def encoded(value):return json.dumps(value,sort_keys=True,separators=(',',':')).encode()
def sealed(value):return encoded({**value,'hash':h(encoded(value))})


class NativeCaptureTests(unittest.TestCase):
    def fixture(self):
        row={'event':{'type':'gap','exchangeId':'x'},'sequence':21,'previous':'a'*64};raw=sealed(row)+b'\n';head=sealed({'sequence':21,'previous':json.loads(raw)['hash'],'manifestHash':'b'*64})
        a={'kind':'native_anchor','sequence':21,'previous':json.loads(raw)['hash'],'start_offset':100,'file':'events.jsonl','identity':[1,2],
           'head_raw_base64':base64.b64encode(head).decode(),'head_raw_sha256':h(head)}
        x={'event':{'type':'book','book':{'price':0.165}},'sequence':22,'previous':a['previous']};r=sealed(x)+b'\n';e={'kind':'native_row','file':a['file'],'identity':a['identity'],'start_offset':100,'end_offset':100+len(r),'original_sequence':22,'original_hash':json.loads(r)['hash'],'raw_sha256':h(r),'raw_base64':base64.b64encode(r).decode()}
        return a,e

    def test_exact_decimal_row_and_anchored_nonzero_sequence(self):
        a,e=self.fixture();v=NativeVerifier();v.accept(a);v.accept(e);self.assertEqual(v.sequence,22);self.assertEqual(v.rows,1)

    def test_changed_hash_offset_identity_and_reanchor_are_rejected(self):
        a,e=self.fixture()
        for key,value in [('identity',[1,3]),('start_offset',101),('original_sequence',23),('raw_sha256','f'*64)]:
            v=NativeVerifier();v.accept(a);z=copy.deepcopy(e);z[key]=value
            with self.assertRaises(ValueError):v.accept(z)
        v=NativeVerifier();v.accept(a)
        with self.assertRaises(ValueError):v.accept(a)

    def test_anchor_excludes_a_complete_but_uncommitted_append(self):
        with tempfile.TemporaryDirectory()as d:
            p=Path(d);r1=sealed({'event':{'type':'tick'},'sequence':1,'previous':''})+b'\n';one=json.loads(r1)
            r2=sealed({'event':{'type':'tick'},'sequence':2,'previous':one['hash']})+b'\n'
            (p/'events.jsonl').write_bytes(r1+r2);(p/'head.json').write_bytes(sealed({'sequence':1,'previous':one['hash'],'manifestHash':'b'*64}))
            a=anchor(p);self.assertEqual(a['start_offset'],len(r1));self.assertEqual(a['original_prefix_sha256'],h(r1))


if __name__=='__main__':unittest.main()
