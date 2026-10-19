"""Retain native-source bytes with first-laptop availability and anchored hashes."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import sys
import time
from moloch.research.verify_learning_transport import source_hash


class NativeVerifier:
    def __init__(self):self.anchor=None;self.offset=None;self.sequence=None;self.previous=None;self.rows=0;self.ending=None
    def accept(self,event):
        kind=event['kind']
        if kind=='native_anchor':
            if self.anchor is not None:raise ValueError('NATIVE_CAPTURE_REANCHOR_FORBIDDEN')
            raw=base64.b64decode(event['head_raw_base64'],validate=True);head=json.loads(raw)
            if hashlib.sha256(raw).hexdigest()!=event['head_raw_sha256']:raise ValueError('NATIVE_ANCHOR_BYTES_CHANGED')
            value={k:v for k,v in head.items()if k!='hash'}
            if hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':')).encode()).hexdigest()!=head['hash']:raise ValueError('NATIVE_ANCHOR_HEAD_HASH')
            if head['sequence']!=event['sequence']or head['previous']!=event['previous']or event['start_offset']<=0:raise ValueError('NATIVE_ANCHOR_SCOPE')
            self.anchor=event;self.offset=event['start_offset'];self.sequence=event['sequence'];self.previous=event['previous']
        elif kind=='native_row':
            if self.anchor is None or self.ending is not None:raise ValueError('NATIVE_ROW_OUTSIDE_CAPTURE')
            raw=base64.b64decode(event['raw_base64'],validate=True);row=json.loads(raw)
            if (event['file'],event['identity'],event['start_offset'])!=(self.anchor['file'],self.anchor['identity'],self.offset):raise ValueError('NATIVE_SOURCE_IDENTITY_OR_OFFSET')
            if event['end_offset']!=self.offset+len(raw)or hashlib.sha256(raw).hexdigest()!=event['raw_sha256']:raise ValueError('NATIVE_ORIGINAL_BYTES_CHANGED')
            source_hash(raw,row)
            if row['sequence']!=self.sequence+1 or row['previous']!=self.previous or row['sequence']!=event['original_sequence']or row['hash']!=event['original_hash']:raise ValueError('NATIVE_CHAIN_DISCONTINUITY')
            self.offset=event['end_offset'];self.sequence=row['sequence'];self.previous=row['hash'];self.rows+=1
        elif kind=='capture_end':
            if self.anchor is None or self.ending is not None:raise ValueError('NATIVE_CAPTURE_END_SCOPE')
            self.ending=event
        else:raise ValueError('UNKNOWN_NATIVE_CAPTURE_EVENT')


def main():
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);args=p.parse_args()
    args.output.mkdir(parents=True,exist_ok=False);v=NativeVerifier();sequence=0;previous='0'*64;last=0;digest=hashlib.sha256()
    with(args.output/'receipt-stream.jsonl').open('xb')as out:
        for line in sys.stdin.buffer:
            if len(line)>24*1024*1024:raise ValueError('NATIVE_CAPTURE_ENVELOPE_BOUND')
            wall=time.time_ns()//1000000;mono=time.monotonic_ns()
            if wall<last:raise ValueError('NATIVE_RECEIVER_CLOCK_REGRESSION')
            event=json.loads(line);v.accept(event);last=wall;sequence+=1
            row={'sequence':sequence,'previous':previous,'observed_at_ms':wall,'observed_monotonic_ns':mono,'event':event}
            previous=hashlib.sha256(json.dumps(row,sort_keys=True,separators=(',',':')).encode()).hexdigest();row['hash']=previous
            raw=(json.dumps(row,separators=(',',':'))+'\n').encode();out.write(raw);out.flush();digest.update(raw)
    result={'classification':'verified_native_byte_tail_not_receiver_semantic_replay_or_execution_authority','receiver_rows':sequence,
            'original_native_rows':v.rows,'terminal_receiver_hash':previous,'transport_sha256':digest.hexdigest(),
            'anchor':v.anchor,'terminal_native_sequence':v.sequence,'terminal_native_hash':v.previous,'end':v.ending,
            'capture_complete':v.ending is not None and v.ending['reason']=='duration_complete','account_requests':0,
            'prefix_semantic_replay_on_receiver':False,'provider_tape_complete':False,'execution_authority':False}
    (args.output/'capture-result.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')


if __name__=='__main__':main()
