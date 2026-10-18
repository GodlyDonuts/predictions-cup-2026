"""Finite read-only filesystem tail of one existing SIG worker's breadth.

Run on the worker host through SSH. This imports no SDK and performs no network
or account operations. Original bytes travel as base64, without JSON retyping.
Each source rotation is an explicit information/continuity barrier.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import time

ROOT=Path('/home/archon/apps/moloch/shared/live-production-v1')
MAX_ROW=8*1024*1024


def main():
    p=argparse.ArgumentParser();p.add_argument('--pid',type=int,required=True);p.add_argument('--start-ticks',required=True)
    p.add_argument('--duration-seconds',type=int,default=5400);args=p.parse_args()
    if args.pid<=0 or not 1<=args.duration_seconds<=7200:raise ValueError('FINITE_EXISTING_WORKER_REQUIRED')
    process=Path('/proc')/str(args.pid);deadline=time.monotonic()+args.duration_seconds
    def alive():return process.exists()and process.joinpath('stat').read_text().split(') ',1)[1].split()[19]==args.start_ticks
    def emit(event):print(json.dumps(event,separators=(',',':')),flush=True)
    emit({'kind':'capture_start','producer_pid':args.pid,'producer_start_ticks':args.start_ticks,
          'reader_source_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'account_requests':0})
    active=None;offset=0;identity=None;reason='duration_complete'
    while time.monotonic()<deadline:
        if not alive():reason='producer_stopped_or_changed';break
        bases=[q for q in ROOT.glob(f'breadth-{args.pid}-*.jsonl')if '-part-'not in q.name]
        if len(bases)>1:raise ValueError('MULTIPLE_SOURCE_BASES_FOR_EXACT_PROCESS')
        if not bases:time.sleep(.1);continue
        base=bases[0]
        files=[base]+sorted(ROOT.glob(base.stem+'-part-*.jsonl'),key=lambda q:int(q.stem.rsplit('-',1)[-1]))
        if active is None:active=base
        if active not in files:raise ValueError('ORIGINAL_SOURCE_DISAPPEARED')
        meta=active.lstat()
        if active.is_symlink()or not active.is_file():raise ValueError('ORIGINAL_REGULAR_SOURCE_REQUIRED')
        current=(meta.st_dev,meta.st_ino)
        if identity is None:
            identity=current;emit({'kind':'file_open','file':active.name,'identity':list(identity),'continuity_verified':False})
        if current!=identity or meta.st_size<offset:raise ValueError('SOURCE_REPLACED_OR_TRUNCATED')
        consumed=False
        with active.open('rb')as source:
            source.seek(offset)
            while True:
                raw=source.readline(MAX_ROW+1)
                if len(raw)>MAX_ROW:raise ValueError('SOURCE_ROW_BOUND')
                if not raw or not raw.endswith(b'\n'):break
                value=json.loads(raw)
                emit({'kind':'source_row','file':active.name,'identity':list(identity),'start_offset':offset,
                      'end_offset':offset+len(raw),'raw_sha256':hashlib.sha256(raw).hexdigest(),
                      'original_sequence':value['sequence'],'original_hash':value['hash'],
                      'raw_base64':base64.b64encode(raw).decode()})
                offset+=len(raw);consumed=True
        index=files.index(active)
        if index+1<len(files)and active.stat().st_size==offset:
            emit({'kind':'source_gap','reason':'unverified_source_segment_transition','file':active.name,'end_offset':offset})
            active=files[index+1];offset=0;identity=None;continue
        if not consumed:time.sleep(.05)
    emit({'kind':'capture_end','reason':reason,'account_requests':0,'producer_tape_completeness_verified':False})


if __name__=='__main__':main()
