"""Post-deployment filesystem-only observer. No SDK, HTTP, replay or tracing."""
import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import time

BASE_SOURCE_SHA256 = '537b7e3e3da0cc4035876fc8232168b9fecd796779e5897ce6fefecd903eba56'
BASELINE_SHA256 = 'd44c87ab55e83e1308b4d04ac9ad2b0087464fc2e7f515c4bb1c40c1830e0173'


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def load_base():
    path = Path(__file__).resolve().parent.parent / 'inspect.py'
    if sha(path.read_bytes()) != BASE_SOURCE_SHA256:
        raise ValueError('BASE_OBSERVER_SOURCE_CHANGED')
    spec = importlib.util.spec_from_file_location('original_hotpath_observer', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def original_json(path, maximum=8*1024*1024):
    # Do not follow user/runtime-selected symlinks or read a moving huge file.
    st = path.lstat()
    if not stat.S_ISREG(st.st_mode) or st.st_size > maximum:
        raise ValueError('ORIGINAL_FILE_BOUND')
    with path.open('rb') as handle:
        opened = os.fstat(handle.fileno())
        if (st.st_dev, st.st_ino) != (opened.st_dev, opened.st_ino):
            raise ValueError('ORIGINAL_FILE_CHANGED')
        raw = handle.read(maximum+1)
        final = os.fstat(handle.fileno())
        if len(raw)>maximum or (opened.st_size,opened.st_mtime_ns)!=(final.st_size,final.st_mtime_ns):
            raise ValueError('ORIGINAL_FILE_CHANGED')
    return json.loads(raw), {'bytes':len(raw),'sha256':sha(raw)}


def pinned_source(path, expected_sha, expected_commit):
    value, boundary = original_json(path, 4*1024*1024)
    if boundary['sha256'] != expected_sha or value.get('sourceCommit') != expected_commit:
        raise ValueError('SOURCE_MANIFEST_PIN')
    return {'sourceCommit':expected_commit, **boundary}


def executable_identity(pid, expected_sha):
    # /proc/PID/exe deliberately resolves the actual running executable, not a
    # release-name path that could select another artifact. No argv/environ read.
    with Path(f'/proc/{pid}/exe').open('rb') as handle:
        before=os.fstat(handle.fileno())
        if not stat.S_ISREG(before.st_mode) or before.st_size>128*1024*1024:
            raise ValueError('EXECUTABLE_BOUND')
        digest=hashlib.sha256()
        for chunk in iter(lambda:handle.read(65536),b''):digest.update(chunk)
        after=os.fstat(handle.fileno())
    if (before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns)!=(after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns):
        raise ValueError('EXECUTABLE_CHANGED')
    if digest.hexdigest()!=expected_sha:raise ValueError('RUNNING_EXECUTABLE_SHA')
    return {'sha256':digest.hexdigest(),'bytes':before.st_size,'device':before.st_dev,'inode':before.st_ino}


def diagnostic(value, boundary, pid, since_ms, captured_ms, base):
    if value.get('pid')!=pid:return {'accepted':False,'reason':'different_or_missing_pid',**boundary}
    try:at=base.clock(value['at'])
    except (KeyError,ValueError):return {'accepted':False,'reason':'missing_original_clock',**boundary}
    if at<since_ms:return {'accepted':False,'reason':'before_readiness',**boundary}
    if at>captured_ms:return {'accepted':False,'reason':'future_original_clock',**boundary}
    safe={k:value[k] for k in ['at','pid','phase','completedReadScopes','cycles','realtimeHealthy','actualOrders','terminalOrders','actualFills','requiredScopeCount','maximumDiscoveryReads'] if k in value}
    selected=value.get('selectedReadScopes')
    if isinstance(selected,list):safe['selectedReadScopeCount']=len(selected)
    elif isinstance(selected,int):safe['selectedReadScopeCount']=selected
    return {'accepted':True,'originalClockAgeMs':captured_ms-at,**boundary,**safe}


def diagnostics(root,pid,since_ms,base):
    values={}
    for name in ['progress.json','status.json','discovery-schedule.json']:
        try:value,boundary=original_json(root/name)
        except FileNotFoundError:values[name]={'accepted':False,'reason':'missing'};continue
        except ValueError as error:
            if str(error)!='ORIGINAL_FILE_CHANGED':raise
            values[name]={'accepted':False,'reason':'concurrent_atomic_replacement'};continue
        completed=datetime.now(timezone.utc).isoformat()
        values[name]={**diagnostic(value,boundary,pid,since_ms,base.clock(completed),base),'readCompletedAt':completed}
    return {'capturedAt':datetime.now(timezone.utc).isoformat(),'files':values,'atomicMultiFileSnapshot':False}


def checked_delta(a,b,base):
    if a['ticksPerSecond']!=b['ticksPerSecond']:raise ValueError('CLOCK_TICK_CHANGED')
    for k in ['userTicks','systemTicks']:
        if b[k]<a[k]:raise ValueError('PROCESS_COUNTER_REGRESSION')
    for k in ['syscw','wchar','write_bytes','read_bytes']:
        if b['io'][k]<a['io'][k]:raise ValueError('PROCESS_COUNTER_REGRESSION')
    return base.proc_delta(a,b)


def progress_change(a,b):
    # A rollover cannot be reconstructed from only overwritten diagnostics.
    first=a['files']['progress.json'];last=b['files']['progress.json']
    statuses=[s['files']['status.json'] for s in [a,b]]
    complete=[s.get('cycles') if s.get('accepted') else None for s in statuses]
    out={'completedCyclesAtSampleBoundaries':complete,'completedCycleDelta':None,
         'scopesCompletedWithinSameSelection':None}
    if all(isinstance(x,int) for x in complete):
        if complete[1]<complete[0]:raise ValueError('CYCLE_COUNTER_REGRESSION')
        out['completedCycleDelta']=complete[1]-complete[0]
    if first.get('accepted') and last.get('accepted') and complete[0] is not None and complete[0]==complete[1] and first.get('selectedReadScopeCount')==last.get('selectedReadScopeCount'):
        start,end=first.get('completedReadScopes'),last.get('completedReadScopes')
        if isinstance(start,int) and isinstance(end,int) and end>=start:
            out['scopesCompletedWithinSameSelection']=end-start
    return out


def comparison(baseline,current,baseline_sha):
    old=baseline['processDeltas'][-1];new=current['processDeltas'][-1]
    old_seconds,new_seconds=old['elapsedSeconds'],new['elapsedSeconds']
    metric=lambda d,sec:{'oneCoreCpuPercent':d['oneCoreCpuPercent'],'writeSyscallsPerSecond':d['writeSyscalls']/sec,
                         'writeCharsPerSecond':d['writeChars']/sec,'bytesPerWriteSyscall':d['bytesPerWriteSyscall']}
    return {'baselineSha256':baseline_sha,'baselinePid':baseline['pid'],'currentPid':current['pid'],
            'baselineSampleSeconds':old_seconds,'currentSampleSeconds':new_seconds,
            'approximatelyMatchingTenSecondSamples':all(9.5<=v<=10.5 for v in [old_seconds,new_seconds]),
            'baseline':metric(old,old_seconds),'current':metric(new,new_seconds),
            'baselineEventWindow':{'notBefore':baseline['notBefore'],'cutoff':baseline['observedCutoff']},
            'currentEventWindow':{'notBefore':current['notBefore'],'cutoff':current['observedCutoff']},
            'wholeLoopSpeedupEstablished':False,
            'limitations':['Different event windows, market activity, pending orders and selected scopes may confound process/latency comparisons.',
                           'CPU samples are bursts, not steady-state throughput. A completed-cycle count is not inferred from a stale status.',
                           'Receipt spans exclude pre-admission waits and include HTTP, validation, durable completion and possibly pagination.',
                           'processedAt precedes accept_batch completion. It measures observation-to-processing start, not durable commit latency.']}


def capture(root,pid,since,source_commit,binary_sha,source_path,source_sha,baseline_path):
    base=load_base();since_ms=base.clock(since)
    baseline,baseline_boundary=original_json(baseline_path)
    if baseline_boundary['sha256']!=BASELINE_SHA256:raise ValueError('ORIGINAL_BASELINE_CHANGED')
    source=pinned_source(source_path,source_sha,source_commit)
    initial=base.proc_sample(pid);binary=executable_identity(pid,binary_sha)
    if base.proc_sample(pid)['startTicks']!=initial['startTicks']:raise ValueError('PID_REUSED_DURING_IDENTITY')
    before=diagnostics(root,pid,since_ms,base)
    a=base.proc_sample(pid);time.sleep(5);b=base.proc_sample(pid);time.sleep(5);c=base.proc_sample(pid)
    after=diagnostics(root,pid,since_ms,base)
    cutoff=datetime.now(timezone.utc).isoformat();cutoff_ms=base.clock(cutoff)
    raw,boundary=base.bounded_tail(root/'bank-all-markets-v1/events.jsonl',16*1024*1024)
    journal=base.journal_summary(raw,since_ms,cutoff_ms)
    dispatch_raw,dispatch_boundary=base.bounded_tail(root/'dispatch.ndjson',2*1024*1024)
    dispatch={**base.dispatch_summary(dispatch_raw,since_ms,cutoff_ms),'sourceBoundary':dispatch_boundary}
    candidates=[]
    for entry in os.scandir(root/'receipts'):
        if entry.name.endswith('.json') and entry.is_file(follow_symlinks=False):
            st=entry.stat(follow_symlinks=False)
            if since_ms<=st.st_mtime*1000 and st.st_size<=8*1024*1024:candidates.append((st.st_mtime,Path(entry.path)))
    chosen=sorted(candidates,reverse=True)[:2048];receipts=[];digest=hashlib.sha256();receipt_bytes=0
    for _,path in sorted(chosen):
        if receipt_bytes+path.stat().st_size>64*1024*1024:break
        value,proof=original_json(path);receipt_bytes+=proof['bytes']
        receipts.append({k:value[k] for k in ['endpoint','requestStartedAt','receivedAt','status'] if k in value})
        digest.update(bytes.fromhex(proof['sha256']))
    final=base.proc_sample(pid)
    if final['startTicks']!=initial['startTicks']:raise ValueError('PID_REUSED_DURING_CAPTURE')
    current_binary=Path(f'/proc/{pid}/exe').stat()
    if (current_binary.st_dev,current_binary.st_ino,current_binary.st_size)!=(binary['device'],binary['inode'],binary['bytes']):
        raise ValueError('EXECUTABLE_CHANGED_DURING_CAPTURE')
    pinned_source(source_path,source_sha,source_commit)
    result={'schemaVersion':1,'state':'observed','pid':pid,'notBefore':since,'observedCutoff':cutoff,
            'identity':{'sourceManifest':source,'actualRunningExecutable':binary},
            'sourceBoundary':boundary,'journal':journal,'dispatch':dispatch,
            'diagnosticSamples':[before,after],'progressChange':progress_change(before,after),
            'processSamples':[a,b,c],'processDeltas':[checked_delta(a,b,base),checked_delta(b,c,base),checked_delta(a,c,base)],
            'receipts':{**base.receipt_summary(receipts,since_ms,cutoff_ms),'selectedFiles':len(receipts),'availableRecentFiles':len(candidates),
                        'selectedOriginalBytes':receipt_bytes,'maximumAggregateOriginalBytes':64*1024*1024,
                        'selectionWasTruncated':len(candidates)>len(receipts),'concatenatedRawFileSha256DigestSha256':digest.hexdigest()},
            'accountRequests':0,'financialReplayPerformed':False,'runtimeMutated':False}
    result['comparison']=comparison(baseline,result,baseline_boundary['sha256'])
    return result


def output_allowed(path,root,baseline,source):
    resolved=path.resolve()
    if resolved.is_relative_to(root.resolve()) or resolved in [baseline.resolve(),source.resolve()]:
        raise ValueError('OUTPUT_MUST_NOT_REPLACE_INPUT_OR_RUNTIME')


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--root',type=Path,required=True);p.add_argument('--pid',type=int,required=True)
    p.add_argument('--since',required=True);p.add_argument('--source-commit',required=True);p.add_argument('--binary-sha256',required=True)
    p.add_argument('--source-manifest',type=Path,required=True);p.add_argument('--source-manifest-sha256',required=True)
    p.add_argument('--baseline',type=Path,required=True);p.add_argument('--output',type=Path,required=True);args=p.parse_args()
    output_allowed(args.output,args.root,args.baseline,args.source_manifest)
    result=capture(args.root,args.pid,args.since,args.source_commit,args.binary_sha256,args.source_manifest,args.source_manifest_sha256,args.baseline)
    raw=(json.dumps(result,sort_keys=True,indent=2)+'\n').encode()
    with args.output.open('xb') as f:f.write(raw)
    print(json.dumps({'outputBytes':len(raw),'sha256':sha(raw)}))
