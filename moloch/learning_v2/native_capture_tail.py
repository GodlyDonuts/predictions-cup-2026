"""Finite file-only capture of the sole worker's committed native event tail.

No SDK, credentials, account requests or financial mutations. Wait for this
exact process to restore, anchor the existing committed prefix, and follow its
original bytes. The prefix digest is provenance, not local semantic replay.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import time

ROOT=Path('/home/archon/apps/moloch/shared/live-production-v1')
MAX_ROW=16*1024*1024


def anchor(bank):
    head_raw=(bank/'head.json').read_bytes();head=json.loads(head_raw)
    row={k:v for k,v in head.items()if k!='hash'}
    if hashlib.sha256(json.dumps(row,sort_keys=True,separators=(',',':')).encode()).hexdigest()!=head['hash']:
        raise ValueError('NATIVE_HEAD_HASH_CHANGED')
    p=bank/'events.jsonl';meta=p.lstat()
    if p.is_symlink()or not p.is_file():raise ValueError('ORIGINAL_NATIVE_REGULAR_JOURNAL_REQUIRED')
    begin=max(0,meta.st_size-2*MAX_ROW);offset=begin;found=None
    with p.open('rb')as f:
        f.seek(begin);lines=f.read(meta.st_size-begin).splitlines(keepends=True)
    for i,line in enumerate(lines):
        offset+=len(line)
        if begin and i==0:continue
        if not line.endswith(b'\n'):break
        x=json.loads(line)
        if x['sequence']==head['sequence']:
            if x['hash']!=head['previous']:raise ValueError('NATIVE_HEAD_JOURNAL_MISMATCH')
            found=offset;break
    if found is None:raise ValueError('COMMITTED_NATIVE_ANCHOR_NOT_IN_BOUNDED_TAIL')
    digest=hashlib.sha256();remaining=found
    with p.open('rb')as f:
        while remaining:
            raw=f.read(min(remaining,1024*1024))
            if not raw:raise ValueError('NATIVE_PREFIX_TRUNCATED')
            digest.update(raw);remaining-=len(raw)
    if (p.stat().st_dev,p.stat().st_ino)!=(meta.st_dev,meta.st_ino):raise ValueError('NATIVE_JOURNAL_REPLACED')
    return {'kind':'native_anchor','file':'bank-all-markets-v1/events.jsonl','identity':[meta.st_dev,meta.st_ino],
            'start_offset':found,'sequence':head['sequence'],'previous':head['previous'],
            'head_raw_sha256':hashlib.sha256(head_raw).hexdigest(),'head_raw_base64':base64.b64encode(head_raw).decode(),
            'original_prefix_sha256':digest.hexdigest(),'prefix_semantic_replay_on_receiver':False,'account_requests':0}


def main():
    p=argparse.ArgumentParser();p.add_argument('--pid',type=int,required=True);p.add_argument('--start-ticks',required=True)
    p.add_argument('--duration-seconds',type=int,default=7200);p.add_argument('--wait-seconds',type=int,default=3600);args=p.parse_args()
    if args.pid<=0 or not 1<=args.duration_seconds<=14400 or not 0<=args.wait_seconds<=3600:raise ValueError('FINITE_EXACT_PRODUCER_REQUIRED')
    process=Path('/proc')/str(args.pid)
    def alive():
        if not process.exists():return False
        state=process.joinpath('stat').read_text().split(') ',1)[1].split()
        return state[19]==args.start_ticks and state[0]!='Z'
    end=time.monotonic()+args.wait_seconds
    while alive():
        x=json.loads((ROOT/'progress.json').read_text())
        if x['pid']==args.pid and x['phase']=='trading'and x.get('realtimeHealthy')is True:break
        if time.monotonic()>=end:raise ValueError('EXACT_PRODUCER_NOT_RESTORED_WITHIN_WAIT')
        time.sleep(.5)
    if not alive():raise ValueError('EXACT_PRODUCER_STOPPED_BEFORE_ANCHOR')
    def emit(x):print(json.dumps(x,separators=(',',':')),flush=True)
    a=anchor(ROOT/'bank-all-markets-v1');a.update(producer_pid=args.pid,producer_start_ticks=args.start_ticks,
             reader_source_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest());emit(a)
    offset=a['start_offset'];journal=ROOT/a['file'];deadline=time.monotonic()+args.duration_seconds;reason='duration_complete'
    while time.monotonic()<deadline:
        if not alive():reason='producer_stopped_or_changed';break
        meta=journal.lstat()
        if journal.is_symlink()or [meta.st_dev,meta.st_ino]!=a['identity']or meta.st_size<offset:raise ValueError('NATIVE_JOURNAL_REPLACED_OR_TRUNCATED')
        consumed=False
        with journal.open('rb')as f:
            f.seek(offset)
            while True:
                raw=f.readline(MAX_ROW+1)
                if len(raw)>MAX_ROW:raise ValueError('NATIVE_ROW_BOUND')
                if not raw or not raw.endswith(b'\n'):break
                if not alive():reason='producer_stopped_or_changed';break
                x=json.loads(raw)
                # A complete append may precede its actual committed head.
                head=json.loads((ROOT/'bank-all-markets-v1/head.json').read_text())
                if x['sequence']>head['sequence']:break
                emit({'kind':'native_row','file':a['file'],'identity':a['identity'],'start_offset':offset,
                      'end_offset':offset+len(raw),'original_sequence':x['sequence'],'original_hash':x['hash'],
                      'raw_sha256':hashlib.sha256(raw).hexdigest(),'raw_base64':base64.b64encode(raw).decode()})
                offset+=len(raw);consumed=True
        if reason!='duration_complete':break
        if not consumed:time.sleep(.05)
    emit({'kind':'capture_end','reason':reason,'account_requests':0,'provider_tape_complete':False})


if __name__=='__main__':main()
