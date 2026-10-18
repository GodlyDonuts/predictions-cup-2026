"""Timestamp original SSH-delivered records at first laptop observation."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import sys
import time


def main():
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);args=p.parse_args()
    args.output.mkdir(parents=True,exist_ok=False);path=args.output/'receipt-stream.jsonl'
    sequence=0;previous='0'*64;last_wall=0;last_source=None;offsets={};ending=None
    with path.open('xb')as dst:
        for line in sys.stdin.buffer:
            wall=time.time_ns()//1000000;mono=time.monotonic_ns()
            if wall<last_wall:raise ValueError('RECEIVER_WALL_CLOCK_REGRESSION')
            last_wall=wall
            if len(line)>12*1024*1024:raise ValueError('SOURCE_ENVELOPE_BOUND')
            event=json.loads(line)
            if event['kind']=='source_row':
                raw=base64.b64decode(event['raw_base64'],validate=True)
                if hashlib.sha256(raw).hexdigest()!=event['raw_sha256']or not raw.endswith(b'\n'):raise ValueError('SOURCE_BYTES_CHANGED')
                source=json.loads(raw);key=event['file']
                if event['start_offset']!=offsets.get(key,0)or event['end_offset']!=event['start_offset']+len(raw):raise ValueError('SOURCE_PREFIX_DISCONTINUITY')
                if event['start_offset']==0 and (source['sequence']!=1 or source['previous']!=''):raise ValueError('ORIGINAL_SEGMENT_ANCHOR_REQUIRED')
                if source['sequence']!=event['original_sequence']or source['hash']!=event['original_hash']:raise ValueError('ORIGINAL_ROW_ID_CHANGED')
                if last_source and last_source[0]==key and (source['sequence']!=last_source[1]+1 or source['previous']!=last_source[2]):raise ValueError('SOURCE_DECLARED_CHAIN_DISCONTINUITY')
                last_source=(key,source['sequence'],source['hash']);offsets[key]=event['end_offset']
            if event['kind']=='capture_end':ending=event
            sequence+=1
            row={'sequence':sequence,'previous':previous,'observed_at_ms':wall,'observed_monotonic_ns':mono,'event':event}
            previous=hashlib.sha256(json.dumps(row,sort_keys=True,separators=(',',':')).encode()).hexdigest();row['hash']=previous
            dst.write((json.dumps(row,separators=(',',':'))+'\n').encode());dst.flush()
    result={'schema_version':1,'classification':'original_record_transport_capture_not_execution_validation',
            'rows':sequence,'terminal_hash':previous,'source_prefix_offsets':offsets,'end':ending,
            'capture_complete':ending is not None and ending.get('reason')=='duration_complete',
            'raw_file_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
            'original_canonical_row_hash_replay_required':True,'cross_segment_continuity_verified':False,
            'provider_tape_complete':False,'account_requests':0,'execution_authority':False}
    (args.output/'capture-result.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')


if __name__=='__main__':main()
