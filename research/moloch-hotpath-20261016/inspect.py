"""Bounded read-only timing audit; no SDK, network, replay, or process tracing."""
import argparse
from collections import Counter, defaultdict
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import time


def clock(value):
    if not isinstance(value, str):
        raise ValueError("missing clock")
    return round(datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp() * 1000)


def stats(values):
    values = sorted(values)
    def q(p):
        return values[max(0, math.ceil(len(values) * p) - 1)] if values else None
    return {"count": len(values), "min": q(0), "median": q(.5), "p90": q(.9), "max": q(1)}


def endpoint_class(endpoint):
    # No account IDs, order IDs or query values leave this classifier.
    if "/fills" in endpoint: return "owned_fills"
    if "/orderbook" in endpoint: return "book"
    if "/trades" in endpoint: return "public_trades"
    if "/nodes" in endpoint: return "contract"
    if "/positions" in endpoint: return "positions"
    if endpoint.startswith("/orders/"): return "owned_order"
    if endpoint.startswith("/orders"): return "open_orders"
    if "/account" in endpoint: return "account"
    if "/collateral" in endpoint: return "collateral"
    if endpoint.startswith("/tournaments/"): return "tournament"
    if endpoint.startswith("/markets/"): return "market"
    return "other"


def union_ms(intervals):
    total = 0
    end = None
    for a, b in sorted(intervals):
        if b < a: raise ValueError("reversed interval")
        total += b - max(a, end if end is not None else a) if end is None or b > end else 0
        end = max(b, end if end is not None else b)
    return total


def receipt_summary(receipts, since, cutoff):
    spans = defaultdict(list)
    intervals = []
    invalid = 0
    for r in receipts:
        try:
            a, b = clock(r["requestStartedAt"]), clock(r["receivedAt"])
            if b < a: raise ValueError("reversed")
            if not since <= a <= b <= cutoff: continue
            spans[endpoint_class(r["endpoint"])].append(b-a)
            intervals.append((a, b))
        except (KeyError, ValueError, TypeError): invalid += 1
    return {"byEndpointClassMs": {k: stats(v) for k, v in sorted(spans.items())},
            "invalidClocks": invalid, "receiptWindowUnionMs": union_ms(intervals),
            "firstStartMs": min((a for a, _ in intervals), default=None),
            "lastEndMs": max((b for _, b in intervals), default=None),
            "meaning": "network-start through validation and durable completion; excludes pre-admission wait; aggregate pagination may span multiple calls"}


def sealed_row(raw):
    # Preserve native Rust numeric/string lexemes. Never reserialize event floats.
    text = raw.decode("utf-8").rstrip("\n")
    if not text.startswith('{"event":'): raise ValueError("native key order")
    decoder = json.JSONDecoder()
    _, event_end = decoder.raw_decode(text, len('{"event":'))
    marker = ',"hash":'
    if text[event_end:event_end+len(marker)] != marker: raise ValueError("native seal position")
    claimed, hash_end = decoder.raw_decode(text, event_end+len(marker))
    body = text[:event_end] + text[hash_end:]
    if hashlib.sha256(body.encode()).hexdigest() != claimed: raise ValueError("native seal")
    row = json.loads(text)
    if set(row) != {"event", "hash", "previous", "sequence"}: raise ValueError("row keys")
    return row


def bounded_tail(path, maximum):
    with path.open("rb") as handle:
        end = os.fstat(handle.fileno()).st_size
        handle.seek(max(0, end-maximum))
        if handle.tell(): handle.readline()
        start = handle.tell()
        raw = handle.read(end-start)
    complete = raw.rfind(b"\n") + 1
    return raw[:complete], {"startByte": start, "endByte": start+complete,
        "observedFileBytes": end, "bytes": complete, "sha256": hashlib.sha256(raw[:complete]).hexdigest(),
        "incompleteTrailingBytesExcluded": len(raw)-complete}


def journal_summary(raw, since, cutoff):
    counts = Counter()
    lag = defaultdict(list)
    groups = Counter()
    first = last = None
    previous = None
    for line in raw.splitlines(keepends=True):
        row = sealed_row(line)
        if previous is not None and (row["previous"] != previous["hash"] or row["sequence"] != previous["sequence"]+1):
            raise ValueError("suffix chain gap")
        if first is None: first = {k: row[k] for k in ("sequence", "hash", "previous")}
        last = {k: row[k] for k in ("sequence", "hash", "previous")}
        previous = row
        e = row["event"]
        try: processed = clock(e.get("processedAt", e.get("receivedAt")))
        except ValueError: continue
        if not since <= processed <= cutoff: continue
        counts[e["type"]] += 1
        groups[processed] += 1
        if "receivedAt" in e:
            received = clock(e["receivedAt"])
            if processed < received: raise ValueError("negative observer lag")
            label = e.get("book", {}).get("transport", e["type"])
            lag[label].append(processed-received)
    times = sorted(groups)
    return {"firstRow": first, "lastRow": last, "suffixRowSealsAndChainVerified": True,
        "fullPrefixVerified": False, "eventCounts": dict(counts),
        "receivedToProcessingStartMs": {k: stats(v) for k, v in sorted(lag.items())},
        "processingTimestampGroups": len(groups), "eventsPerProcessingTimestamp": stats(list(groups.values())),
        "processingTimestampGapMs": stats([b-a for a,b in zip(times,times[1:])]),
        "clockLimitation": "processedAt is stamped before accept_batch, not a durable completion timestamp; timestamp groups are not guaranteed batch boundaries"}


def proc_sample(pid):
    base = Path('/proc')/str(pid)
    # Parse after comm's final ')' so spaces in comm cannot shift the fields.
    fields = (base/'stat').read_text().rsplit(')',1)[1].split()
    io = {k: int(v.strip()) for k,v in (line.split(':',1) for line in (base/'io').read_text().splitlines())}
    return {"utc": datetime.now(timezone.utc).isoformat(), "monotonic": time.monotonic(),
            "ticksPerSecond": os.sysconf('SC_CLK_TCK'), "startTicks": int(fields[19]),
            "userTicks": int(fields[11]), "systemTicks": int(fields[12]),
            "rssBytes": int(fields[21])*os.sysconf('SC_PAGE_SIZE'), "io": io}


def dispatch_summary(raw, since, cutoff):
    counts=Counter(); attempts={}; acknowledged=set(); delays=[]; previous=None
    first=last=None
    for line in raw.splitlines(keepends=True):
        row=sealed_row(line)
        if previous is not None and (row['previous']!=previous['hash'] or row['sequence']!=previous['sequence']+1):
            raise ValueError('dispatch suffix gap')
        if first is None:first={k:row[k] for k in ('sequence','hash','previous')}
        last={k:row[k] for k in ('sequence','hash','previous')};previous=row
        e=row['event'];kind=e.get('kind');r=e.get('receipt',{})
        value=e.get('at',r.get('receivedAt'))
        if value is None:continue
        at=clock(value)
        if not since<=at<=cutoff:continue
        counts[kind]+=1
        if kind=='attempt':attempts[e['qid']]=at
        if kind=='ack':
            acknowledged.add(e['qid'])
            if e['qid'] in attempts:
                delay=clock(r['receivedAt'])-attempts[e['qid']]
                if delay<0:raise ValueError('ACK precedes attempt')
                delays.append(delay)
    return {'firstRow':first,'lastRow':last,'datedEventCounts':dict(counts),
            'attemptToAckReceivedMs':stats(delays),'attemptsWithoutAckInWindow':len(set(attempts)-acknowledged),
            'suffixRowSealsAndChainVerified':True,'fullPrefixVerified':False,
            'limitations':'Undated rows excluded from time-window counts. Unacknowledged suffix attempts are censored, not proof of unknown acceptance. Attempt-to-ACK includes SDK admission, HTTP, validation and completion; it is not provider-only latency.'}


def proc_delta(a,b):
    if a['startTicks'] != b['startTicks']: raise ValueError('PID reused')
    seconds = b['monotonic']-a['monotonic']
    if seconds <= 0: raise ValueError('sample clock')
    user = (b['userTicks']-a['userTicks'])/a['ticksPerSecond']
    system = (b['systemTicks']-a['systemTicks'])/a['ticksPerSecond']
    calls = b['io']['syscw']-a['io']['syscw']
    written = b['io']['wchar']-a['io']['wchar']
    return {"elapsedSeconds": seconds, "userCpuSeconds": user, "systemCpuSeconds": system,
        "oneCoreCpuPercent": 100*(user+system)/seconds, "writeSyscalls": calls,
        "writeChars": written, "bytesPerWriteSyscall": written/calls if calls else None,
        "writeBytes": b['io']['write_bytes']-a['io']['write_bytes'],
        "readBytes": b['io']['read_bytes']-a['io']['read_bytes']}


def run(root,pid,since,sample_seconds=5,maximum_tail=16*1024*1024):
    cutoff = datetime.now(timezone.utc).isoformat()
    cutoff_ms = clock(cutoff)
    raw, boundary = bounded_tail(root/'bank-all-markets-v1/events.jsonl',maximum_tail)
    journal = journal_summary(raw,clock(since),cutoff_ms)
    dispatch_raw,dispatch_boundary=bounded_tail(root/'dispatch.ndjson',2*1024*1024)
    dispatch={**dispatch_summary(dispatch_raw,clock(since),cutoff_ms),'sourceBoundary':dispatch_boundary}
    files = []
    for entry in os.scandir(root/'receipts'):
        if entry.name.endswith('.json') and entry.is_file(follow_symlinks=False):
            st = entry.stat(follow_symlinks=False)
            if st.st_mtime*1000 >= clock(since) and st.st_size <= 8*1024*1024:
                files.append((st.st_mtime,Path(entry.path)))
    chosen = sorted(files,reverse=True)[:2048]
    receipts = []; receipt_digests = hashlib.sha256()
    for _,path in sorted(chosen):
        data = path.read_bytes(); receipt_digests.update(hashlib.sha256(data).digest())
        receipts.append(json.loads(data))
    diagnostics = {}
    for name in ['progress.json','discovery-schedule.json','status.json']:
        path=root/name; data=path.read_bytes(); d=json.loads(data)
        diagnostics[name]={"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest(),
            **{k:d[k] for k in ['at','pid','phase','completedReadScopes','selectedReadScopes','cycles','realtimeHealthy','actualOrders','terminalOrders','actualFills','requiredScopeCount','maximumDiscoveryReads'] if k in d and not isinstance(d[k],list)}}
    a=proc_sample(pid);time.sleep(sample_seconds);b=proc_sample(pid);time.sleep(sample_seconds);c=proc_sample(pid)
    return {"schemaVersion":1,"observedCutoff":cutoff,"notBefore":since,"pid":pid,
        "sourceBoundary":boundary,"journal":journal,
        "dispatch":dispatch,
        "receipts":{**receipt_summary(receipts,clock(since),cutoff_ms),"selectedFiles":len(chosen),
                    "availableRecentFiles":len(files),"concatenatedRawFileSha256DigestSha256":receipt_digests.hexdigest()},
        "diagnostics":diagnostics,"processSamples":[a,b,c],"processDeltas":[proc_delta(a,b),proc_delta(b,c),proc_delta(a,c)],
        "financialReplayPerformed":False,"accountRequests":0}


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--root',type=Path,required=True)
    parser.add_argument('--pid',type=int,required=True);parser.add_argument('--since',required=True)
    parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    result=run(args.root,args.pid,args.since)
    args.output.write_text(json.dumps(result,indent=2,sort_keys=True)+'\n')
    print(json.dumps({'outputBytes':args.output.stat().st_size,'sha256':hashlib.sha256(args.output.read_bytes()).hexdigest()}))
