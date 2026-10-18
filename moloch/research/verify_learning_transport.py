"""Verify an immutable prefix of the read-only SIG file transport.

Reconstruct the producer's pre-hash bytes by removing its exact top-level hash
field, without retyping decimal JSON tokens. This is byte/hash verification,
not native semantic replay, source completeness or permission to trade.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def source_hash(raw, row):
    if list(row) != ['event', 'hash', 'previous', 'sequence'] or not raw.endswith(b'\n'):
        raise ValueError('EXACT_PRODUCER_ROW_SHAPE')
    token = (',"hash":"'+row['hash']+'"').encode('ascii')
    if raw.count(token) != 1:
        raise ValueError('EXACT_TOP_LEVEL_HASH_FIELD')
    before_hash = raw[:-1].replace(token, b'', 1)
    if digest(before_hash) != row['hash']:
        raise ValueError('ORIGINAL_PRODUCER_HASH_MISMATCH')


def verify(path, cutoff_ms):
    sequence = 0; previous = '0'*64; last_wall = -1; last_mono = -1
    offsets = {}; originals = {}; opened = {}; source_rows = 0; gaps = 0
    transport_hash = hashlib.sha256(); prefix_bytes = 0; first_wall = None
    # Snapshot a file-size boundary. A growing tail is never silently a sealed
    # whole capture; an incomplete last line is excluded from this prefix.
    boundary = path.stat().st_size
    with path.open('rb') as stream:
        while stream.tell() < boundary:
            raw = stream.readline(min(12*1024*1024+1, boundary-stream.tell()))
            if len(raw)>12*1024*1024:raise ValueError('TRANSPORT_ROW_BOUND')
            if not raw.endswith(b'\n'):break
            row = json.loads(raw)
            if row['observed_at_ms']>cutoff_ms:break
            claimed = row.pop('hash')
            if row['sequence']!=sequence+1 or row['previous']!=previous:
                raise ValueError('RECEIVER_CHAIN_DISCONTINUITY')
            if digest(json.dumps(row,sort_keys=True,separators=(',',':')).encode())!=claimed:
                raise ValueError('RECEIVER_HASH_CHANGED')
            if row['observed_at_ms']<last_wall or row['observed_monotonic_ns']<last_mono:
                raise ValueError('RECEIVER_CLOCK_REGRESSION')
            last_wall=row['observed_at_ms'];last_mono=row['observed_monotonic_ns']
            if first_wall is None:first_wall=last_wall
            event=row['event'];kind=event['kind']
            if kind=='file_open':
                if event['file']in opened:raise ValueError('SOURCE_REOPENED')
                opened[event['file']]=event['identity']
            elif kind=='source_gap':gaps+=1
            elif kind=='source_row':
                name=event['file'];original=base64.b64decode(event['raw_base64'],validate=True)
                if opened.get(name)!=event['identity'] or digest(original)!=event['raw_sha256']:
                    raise ValueError('SOURCE_BYTES_OR_IDENTITY_CHANGED')
                if event['start_offset']!=offsets.get(name,0) or event['end_offset']!=event['start_offset']+len(original):
                    raise ValueError('SOURCE_PREFIX_DISCONTINUITY')
                value=json.loads(original);source_hash(original,value)
                n,prior=originals.get(name,(0,''))
                if value['sequence']!=n+1 or value['previous']!=prior:
                    raise ValueError('ORIGINAL_CHAIN_DISCONTINUITY')
                if value['sequence']!=event['original_sequence'] or value['hash']!=event['original_hash']:
                    raise ValueError('SOURCE_DECLARED_ID_CHANGED')
                originals[name]=(value['sequence'],value['hash']);offsets[name]=event['end_offset'];source_rows+=1
            elif kind not in ['capture_start','capture_end']:
                raise ValueError('UNKNOWN_CAPTURE_EVENT')
            sequence=row['sequence'];previous=claimed;transport_hash.update(raw);prefix_bytes+=len(raw)
    if not source_rows:raise ValueError('NO_ORIGINAL_SOURCE_ROWS')
    return {'classification':'verified_original_byte_hash_prefix_not_native_semantic_or_execution_validation',
            'source_file':path.name,'cutoff_ms':cutoff_ms,'transport_prefix_bytes':prefix_bytes,
            'transport_prefix_sha256':transport_hash.hexdigest(),'receiver_rows':sequence,
            'receiver_terminal_hash':previous,'first_observed_ms':first_wall,'last_observed_ms':last_wall,
            'original_source_rows':source_rows,'source_prefix_offsets':offsets,
            'source_chain_terminals':originals,'declared_source_gaps':gaps,
            'original_producer_row_hashes_verified':True,'native_semantic_replay_verified':False,
            'whole_capture_complete':False,'provider_tape_complete':False,'cross_segment_continuity_verified':False,
            'account_requests':0,'execution_authority':False}


def main():
    p=argparse.ArgumentParser();p.add_argument('--input',type=Path,required=True)
    p.add_argument('--cutoff-ms',type=int,required=True);p.add_argument('--output',type=Path,required=True);args=p.parse_args()
    result=verify(args.input,args.cutoff_ms)
    with args.output.open('x',encoding='utf-8')as f:json.dump(result,f,indent=2);f.write('\n')
    print(json.dumps({'verified_source_rows':result['original_source_rows'],'prefix_bytes':result['transport_prefix_bytes']}))


if __name__=='__main__':main()
