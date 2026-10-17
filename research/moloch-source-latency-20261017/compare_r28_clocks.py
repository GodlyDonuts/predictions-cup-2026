"""Prospective R27/R28 wrapper; unchanged checked clock metrics, no live access."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path

HERE=Path(__file__).resolve().parent
ALGORITHM_LF_SHA='1fbc599e16417778d4aa48545eb2c7af9c3073875fd4ffb26f72c0333443a34b'
GENERATION_READER_LF_SHA='63ca51eb5f4f9d00b25c8518cd2892dbde0f3ff78013929eeff3a57892997e35'
PROTOCOL_LF_SHA='7f73d1cd6a2bfeddbe97539d26722c3f1dd4b84d7aaa9d8c9d3e27b31e950619'
OLD_SHA='d988c4042c34299a3e31fc28c8915590184b4fa681ba02d9d79e11bd8fd5419d'
INCIDENT_SHA='7ee0a95b75500d732e2568a1962eb1491cd525125284a495f39bc14d38666af4'
SOURCE='069a85a478942eb8148c354df368e369e940eb7e'
BINARY='203462d82692c9a68c3303546271bc8b7fc60e3945b36fe4224a5252be779dfe'
NATIVE='5b61d3cbb38eadf5170fafae5c91cb5b75858346b2771031084224bfa2c3d6d4'
CONTROL='64dcee5feb31449d4862f801986f2edac68106e428691529525b70a45bd865d8'
GENERATION='813f85a7a6da473c52152d7acbdd0b88102f288380c95199bd5d23360b0ff12a'
PID=661886
def digest(raw):return hashlib.sha256(raw).hexdigest()
# Known repository readers only. Never execute source supplied in a packet.
if digest((HERE/'compare_book_clocks.py').read_bytes().replace(b'\r\n',b'\n'))!=ALGORITHM_LF_SHA:raise ValueError('R28_ALGORITHM_CHANGED')
reader=HERE/'compare_r27_clocks.py'
if digest(reader.read_bytes().replace(b'\r\n',b'\n'))!=GENERATION_READER_LF_SHA:raise ValueError('R28_GENERATION_READER_CHANGED')
spec=importlib.util.spec_from_file_location('_r28_checked_wrapper',reader)
P=importlib.util.module_from_spec(spec);spec.loader.exec_module(P)
C=P.C
generations=P.generations


def first_observation(raw,progress):
    C.need(raw.endswith(b'\n') and 0<len(raw)<=4*1024**2,'R28_OBSERVER_TRACE_BOUND')
    lines=raw.splitlines();C.need(0<len(lines)<=1024 and all(len(x)<=1024**2 for x in lines),'R28_OBSERVER_ROWS')
    rows=[C.V.load(x) for x in lines];times=[C.N.instant(r['observedAt']) for r in rows]
    C.need(times==sorted(times),'R28_OBSERVER_CHRONOLOGY')
    waiting=[r for r in rows if r.get('phase')=='waiting-for-first-fresh-progress']
    fresh=[r for r in rows if r.get('phase')=='first-fresh-trading']
    complete=[r for r in rows if r.get('phase')=='completed']
    C.need(len(waiting)==len(fresh)==len(complete)==1 and waiting[0]['pid']==PID
           and rows[0]==waiting[0] and rows[-1]==complete[0] and not any(r.get('phase')=='failed' for r in rows),'R28_SINGLE_COMPLETE_FIRST_OBSERVATION')
    C.need(fresh[0]['progress']==progress and progress['pid']==PID and progress['phase']=='trading'
           and progress['realtimeHealthy'] is True,'R28_EXACT_FIRST_PROGRESS')
    start=C.N.instant(progress['at']);observed=C.N.instant(fresh[0]['observedAt'])
    C.need(times[0]<=observed and 0<=observed-start<=120000*C.NS_PER_MS
           and times[-1]>=start+C.WINDOW_NS,'R28_FIRST_OBSERVATION_FRESHNESS_AND_COMPLETION')
    return start,progress['at'],fresh[0]['observedAt']


def rollout(packet):
    r=packet.json('production-rollout.original.json')
    C.need(r['newPid']==PID and r['sourceCommit']==SOURCE and r['binarySha256']==BINARY
           and r['engineIdentity']==NATIVE and r['controlSha256']==CONTROL
           and digest(packet.small(packet.name('control.original.json')))==CONTROL,'R28_EXACT_ROLLOUT_CONTROL')
    status=packet.json('model-status.original.json')
    C.need(status['workerPid']==PID and status['baseControlSha256']==CONTROL
           and status['previousGenerationSha256']==GENERATION,'R28_ACTUAL_SIXTH_GENERATION_CHANNEL')
    return r


def run(a):
    declaration_raw=Path(a.protocol).read_bytes()
    C.need(digest(declaration_raw)==a.protocol_sha256 and digest(declaration_raw.replace(b'\r\n',b'\n'))==PROTOCOL_LF_SHA,'R28_FROZEN_PROTOCOL')
    declaration=C.V.load(declaration_raw)
    C.need(declaration['windowMs']==300000 and declaration['policyMaximumBookAgeMs']==10000
           and declaration['oldOutcomeAlreadyInspected'] is True and declaration['confirmatory'] is False
           and C.WINDOW_NS==300000*C.NS_PER_MS and C.AGE_NS==10000*C.NS_PER_MS,'R28_UNCHANGED_WINDOW_METRICS')
    C.need(a.old_sha256==OLD_SHA and a.incident_sha256==INCIDENT_SHA,'R28_FIXED_ORIGINAL_INPUTS')
    packets=[]
    try:
        old=C.Packet(a.old_packet,a.old_sha256);packets.append(old)
        incident=C.Packet(a.incident_packet,a.incident_sha256);packets.append(incident)
        original,g6=generations(incident);oldraw,g5=generations(old)
        C.need(len(g5)==5 and len(g6)==6 and g6[:5]==g5 and original.startswith(oldraw)
               and g6[-1]['generationSha256']==GENERATION,'R28_ORIGINAL_FIVE_TO_SIX_PREFIX')
        start,text=C.progress(old,'first-trading-progress.original.json',654706)
        C.need(text==declaration['oldWindowStartUtc'] and all(g['atMs']*C.NS_PER_MS<=start for g in g5),'R28_FIXED_OLD_WINDOW')
        result={'protocolRawSha256':a.protocol_sha256,'protocolLfSha256':PROTOCOL_LF_SHA,'algorithmLfSha256':ALGORITHM_LF_SHA,
                'generationReaderLfSha256':GENERATION_READER_LF_SHA,'wrapperRawSha256':digest(Path(__file__).read_bytes()),
                'oldOutcomeAlreadyInspected':True,'confirmatory':False,'comparisonPerformed':False,'new':None,
                'old':C.measure(old,start,text,654706,{'zipSha256':old.expected,**old.original_pin('first-trading-progress.original.json')}),
                'incident':{'zipSha256':incident.expected,'originalFundamental':incident.original_pin('fundamental.original.jsonl'),'generations':g6},
                'oldGenerations':g5,'limitations':declaration['limitations'],
                'executionAuthority':False,'profitabilityEstablished':False,'causalBenefitEstablished':False}
        if not a.old_only:
            new=C.Packet(a.new_packet,a.new_sha256);packets.append(new)
            first=C.Packet(a.new_first_packet,a.new_first_sha256);packets.append(first)
            event_path=Path(a.new_observer_events)
            C.need(event_path.is_file() and not event_path.is_symlink() and event_path.stat().st_size<=4*1024**2,'R28_ORIGINAL_OBSERVER_FILE')
            events=event_path.read_bytes();C.need(digest(events)==a.new_observer_events_sha256,'R28_ORIGINAL_OBSERVER_SHA')
            rollout(new);rollout(first)
            first_raw=first.small(first.name('first-trading-progress.original.json'))
            C.need(first_raw==new.small(new.name('first-trading-progress.original.json')),'R28_ORIGINAL_FIRST_PROGRESS_BYTES')
            start,text,observed=first_observation(events,C.V.load(first_raw))
            first_capture=first.json('capture.json')
            C.need(first_capture['pid']==PID and first_capture['sourceCommit']==SOURCE
                   and C.N.instant(first_capture['observedAt'])>=C.N.instant(observed),'R28_FIRST_PACKET_OBSERVATION_ORDER')
            C.need(first.small(first.name('anchor-head.original.json'))==new.small(new.name('anchor-head.original.json')),'R28_SAME_STOPPED_NATIVE_ANCHOR')
            C.need(first.sealed('head.original.json')['sequence']<=new.sealed('head.original.json')['sequence'],'R28_FIRST_PACKET_BEFORE_FINAL')
            for packet in (first,new):
                current,current_g=generations(packet)
                C.need(current.startswith(original) and len(current_g)==6 and current_g==g6
                       and all(g['atMs']*C.NS_PER_MS<=start for g in current_g),'R28_SIX_ORIGINAL_GENERATIONS_NO_SEVENTH')
                C.need(digest(packet.small(packet.name('bridge.original.json')))=='b54830b219fba35238792927fc0c123c582a91b4311cb86a3b100f305f4c32e8','R28_UNCHANGED_FIFTEEN_BRIDGES')
            result['new']=C.measure(new,start,text,PID,{'zipSha256':first.expected,**first.original_pin('first-trading-progress.original.json')})
            result['new'].update({'firstObservedFreshAt':observed,'observerEventsSha256':a.new_observer_events_sha256,
                'firstPacketZipSha256':first.expected,'firstPacketCaptureOriginal':first.original_pin('capture.json'),
                'productionRolloutOriginal':new.original_pin('production-rollout.original.json'),'generations':g6,
                'wholeOriginalSixGenerationPrefixRetained':True,'noSeventhGeneration':True})
            result['comparisonPerformed']=True
            C.need(event_path.read_bytes()==events,'R28_OBSERVER_CHANGED_DURING_READ')
        for packet in packets:packet.finish()
        C.need(digest(Path(a.protocol).read_bytes())==a.protocol_sha256,'R28_PROTOCOL_CHANGED_DURING_READ')
        return result
    finally:
        for packet in packets:packet.z.close()


def self_test():
    # Synthetic chronology only: no original cohort, clock window or outcomes.
    p={'pid':PID,'phase':'trading','realtimeHealthy':True,'at':'2026-10-17T16:00:00.000Z'}
    rows=[{'observedAt':'2026-10-17T15:59:59.000Z','phase':'waiting-for-first-fresh-progress','pid':PID},
          {'observedAt':'2026-10-17T16:00:01.000Z','phase':'first-fresh-trading','progress':p},
          {'observedAt':'2026-10-17T16:05:01.000Z','phase':'completed'}]
    def raw(r):return b'\n'.join(json.dumps(v).encode() for v in r)+b'\n'
    assert first_observation(raw(rows),p)[1]==p['at']
    cases=[rows+[rows[1]],rows[:1]+[dict(rows[1],observedAt='2026-10-17T16:02:00.001Z')]+rows[2:],
           rows[:1]+[dict(rows[1],observedAt='2026-10-17T15:59:59.500Z')]+rows[2:],rows[:-1],
           rows+[{'observedAt':'2026-10-17T16:05:02.000Z','phase':'failed'}]]
    for case in cases:
        try:first_observation(raw(case),p)
        except ValueError:pass
        else:raise AssertionError('invalid prospective chronology accepted')
    q=dict(p,pid=PID+1)
    try:first_observation(raw(rows),q)
    except ValueError:pass
    else:raise AssertionError('wrong original progress accepted')
    assert C.WINDOW_NS==300000*C.NS_PER_MS and C.AGE_NS==10000*C.NS_PER_MS
    print('PASS synthetic first-observation validation: valid, duplicate, old, future, incomplete, failed, mismatched progress; original metric bounds unchanged')


def main():
    p=argparse.ArgumentParser();p.add_argument('--self-test',action='store_true')
    for n in ('old-packet','old-sha256','incident-packet','incident-sha256','protocol','protocol-sha256','output',
              'new-packet','new-sha256','new-first-packet','new-first-sha256','new-observer-events','new-observer-events-sha256'):
        p.add_argument('--'+n)
    p.add_argument('--old-only',action='store_true');a=p.parse_args()
    if a.self_test:self_test();return
    C.need(all(getattr(a,n) for n in ('old_packet','old_sha256','incident_packet','incident_sha256','protocol','protocol_sha256','output')),'R28_REQUIRED_PINS')
    new_args=[a.new_packet,a.new_sha256,a.new_first_packet,a.new_first_sha256,a.new_observer_events,a.new_observer_events_sha256]
    C.need((a.old_only and not any(new_args)) or (not a.old_only and all(new_args)),'R28_EXPLICIT_COMPLETE_INPUT_MODE')
    out=Path(a.output);C.need(not out.exists(),'R28_OUTPUT_EXISTS');result=run(a)
    with out.open('x',encoding='utf-8',newline='\n')as f:json.dump(result,f,indent=2,allow_nan=False);f.write('\n')
if __name__=='__main__':main()
