"""Metadata-only chronological feasibility audit; never open prices or train.

Clock extents can disprove readiness but cannot establish continuous coverage,
source authenticity, all-market completeness or independent profitability.
"""
import argparse,hashlib,json,os
from dataclasses import dataclass,asdict
from datetime import datetime,timezone
from pathlib import Path
from moloch.learning_realism.protocol import DAY,valid_sha
from .selection import frozen

VENUES=('SIG','Kalshi','Polymarket')


def integer(v):return type(v)is int and v>=0
def utc(ms):return datetime.fromtimestamp(ms/1000,timezone.utc).isoformat().replace('+00:00','Z')


@dataclass(frozen=True)
class Extent:
    venue:str
    first_ms:int
    last_ms:int
    label:str
    source_sha256:str

    def __post_init__(self):
        if(self.venue not in VENUES or not integer(self.first_ms)or not integer(self.last_ms)
            or self.last_ms<self.first_ms or not valid_sha(self.source_sha256)or not isinstance(self.label,str)):
            raise ValueError('VALID_PINNED_CLOCK_EXTENT_REQUIRED')


def union(intervals):
    result=[]
    for lo,hi in sorted(intervals):
        if not result or lo>result[-1][1]:result.append([lo,hi])
        else:result[-1][1]=max(result[-1][1],hi)
    return result


def clock_inventory(path):
    raw=Path(path).read_bytes();value=json.loads(raw);result=[]
    for row in value['datasets']:
        for venue,s in row.get('venues',{}).items():
            if venue in VENUES:
                result.append(Extent(venue,s['first_ms'],s['last_ms'],row['dataset'],row['manifest_sha256']))
    return result,hashlib.sha256(raw).hexdigest()


def sealed_manifest(path):
    path=Path(path);raw=path.read_bytes();sha=hashlib.sha256(raw).hexdigest()
    if json.loads((path.parent/'COMPLETE.json').read_text())['manifestSha256']!=sha:
        raise ValueError('METADATA_MANIFEST_SEAL_MISMATCH')
    value=json.loads(raw);result=[]
    for key,s in value.get('sourceCoverage',{}).items():
        venue=key.rsplit('/',1)[-1]
        if venue in VENUES and s.get('validRows',0):
            result.append(Extent(venue,s['firstAtMs'],s['lastAtMs'],path.parent.name+'/'+key,sha))
    # This verifies only manifest bytes against the local seal, not the original
    # archive, decoder semantics or observation payloads.
    return result,sha


def registry_snapshot(directory):
    root=Path(directory)
    if not root.is_dir():return None,{'present':False,'records':0,'sha256':None}
    paths=sorted(root.glob('*.json'));digest=hashlib.sha256();records=[]
    for p in paths:
        raw=p.read_bytes();row=json.loads(raw)
        if(row.get('kind')not in('train','selection','inspected_development','test')
            or not integer(row.get('start_ms'))or not integer(row.get('end_ms'))
            or row['end_ms']<=row['start_ms']or not valid_sha(row.get('source_sha256'))):
            raise ValueError('VALID_DATA_USE_METADATA_REQUIRED')
        digest.update(p.name.encode()+hashlib.sha256(raw).digest());records.append(row)
    if paths!=sorted(root.glob('*.json')):raise ValueError('DATA_USE_SNAPSHOT_CHANGED_RETRY')
    return records,{'present':True,'records':len(records),'sha256':digest.hexdigest(),
        'unrecorded_manual_or_other_host_exposure_proven_absent':False}


def audit(extents,records,*,cutoff_ms,competition_start_ms=None):
    if not integer(cutoff_ms)or cutoff_ms<=8*DAY:raise ValueError('EXPLICIT_CUTOFF_AFTER_EIGHT_DAYS')
    train_end=cutoff_ms-8*DAY;test_start=cutoff_ms-4*DAY
    if competition_start_ms is not None and(not integer(competition_start_ms)or competition_start_ms>=train_end):
        raise ValueError('DECLARED_COMPETITION_START_BEFORE_TRAIN_END')
    reasons=[];available={}
    for venue in VENUES:
        rows=[e for e in extents if e.venue==venue]
        if not rows:
            available[venue]={'extents':0};reasons.append('NO_INSPECTED_METADATA_FOR_'+venue);continue
        first=min(e.first_ms for e in rows);last=max(e.last_ms for e in rows)
        available[venue]={'extents':len(rows),'first_utc':utc(first),'last_utc':utc(last),
            'first_ms':first,'last_ms':last,'continuous_coverage_established':False,
            'all_market_coverage_established':False}
        if first>=train_end:reasons.append('NO_INSPECTED_'+venue+'_EXTENT_REACHES_TRAIN_PARTITION')
    overlap=[];by_kind={}
    if records is not None:
        for r in records:
            if r['start_ms']<cutoff_ms and test_start<r['end_ms']:
                overlap.append((max(test_start,r['start_ms']),min(cutoff_ms,r['end_ms'])))
                by_kind[r['kind']]=by_kind.get(r['kind'],0)+1
    if overlap:reasons.append('REQUESTED_TEST_CALENDAR_ALREADY_EXPOSED')
    exposed=union(overlap)
    return {'classification':'metadata_feasibility_not_full_coverage_or_performance_qualification',
        'requested_cutoff_utc':utc(cutoff_ms),'requested_cutoff_ms':cutoff_ms,
        'requested_roles_ms':{'train':[competition_start_ms,train_end],
            'evolution':[train_end,test_start],'independent_test':[test_start,cutoff_ms]},
        'competition_start_verified':False,'competition_start_declared':competition_start_ms is not None,
        'available_outer_clock_extents':available,
        'test_exposure':{'local_registry_available':records is not None,'overlapping_records':sum(by_kind.values()),
            'by_kind':by_kind,'union_intervals_ms':exposed,'union_intervals_utc':[[utc(a),utc(b)]for a,b in exposed],
            'union_duration_ms':sum(b-a for a,b in exposed),'already_recorded_exposure':bool(overlap),
            'absence_of_unrecorded_exposure_established':False},
        'verdict':'ineligible_at_requested_cutoff'if reasons else'requires_source_row_and_independence_verification',
        'disqualifying_metadata_findings':reasons,'full_history_ready_proven':False,
        'required_further_evidence':['official competition-start provenance','complete original per-venue/per-market chronology',
            'coverage gaps and replay/execution calibration','frozen unseen four-day test provenance'],
        'prices_or_returns_exported':False,'raw_observation_rows_opened':False,
        'independent_test_opened':False,'account_requests':0,'production_changed':False}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--clock-inventory',type=Path,required=True)
    p.add_argument('--manifest',type=Path,action='append',default=[])
    p.add_argument('--registry',type=Path,default=Path(os.environ.get('LOCALAPPDATA',Path.home()/'.local/share'))/'Moloch/learning-v2/data-use-registry-v1')
    p.add_argument('--cutoff-ms',type=int,required=True);p.add_argument('--competition-start-ms',type=int)
    p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    extents,sha=clock_inventory(a.clock_inventory);inputs=[{'kind':'existing_clock_inventory','sha256':sha}]
    for path in a.manifest:
        extra,sha=sealed_manifest(path);extents.extend(extra);inputs.append({'kind':'sealed_manifest_metadata','dataset':path.parent.name,'sha256':sha})
    records,registry=registry_snapshot(a.registry)
    report=audit(extents,records,cutoff_ms=a.cutoff_ms,competition_start_ms=a.competition_start_ms)
    report.update(observed_utc=datetime.now(timezone.utc).isoformat(),inputs=inputs,data_use_registry=registry,
                  extent_sources=[asdict(e)for e in extents])
    frozen(a.output,report);print(json.dumps({k:report[k]for k in('verdict','disqualifying_metadata_findings','test_exposure')}))


if __name__=='__main__':main()
