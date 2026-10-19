"""Stream compiled-history clock coverage without exposing prices or outcomes."""
import argparse,gzip,hashlib,json
from pathlib import Path
from datetime import datetime,timezone


def clocks(path):
    venues={};rows=0;regressions=0;previous=None;gaps=0
    with gzip.open(path,'rt',encoding='utf8')as f:
        for line in f:
            row=json.loads(line);at=row.get('available_at_ms')
            if type(at)is not int:raise ValueError('ORIGINAL_AVAILABILITY_CLOCK_REQUIRED')
            rows+=1;regressions+=int(previous is not None and at<previous);previous=at
            kind=row.get('kind');gaps+=int(kind in('gap','reference_gap'))
            venue='SIG'if kind=='book'else row.get('venue')if kind=='reference'else None
            if venue not in('SIG','Kalshi','Polymarket'):continue
            s=venues.setdefault(venue,{'observations':0,'first_ms':at,'last_ms':at,'maximum_observation_gap_ms':0})
            s['observations']+=1;s['maximum_observation_gap_ms']=max(s['maximum_observation_gap_ms'],at-s['last_ms'])
            s['first_ms']=min(s['first_ms'],at);s['last_ms']=max(s['last_ms'],at)
    return {'rows':rows,'clock_regressions':regressions,'explicit_gap_rows':gaps,'venues':venues}


def audit(root):
    out=[]
    for manifest in sorted(Path(root).glob('*/dataset-manifest.json')):
        directory=manifest.parent;raw=manifest.read_bytes();value=json.loads(raw)
        row={'dataset':directory.name,'manifest_sha256':hashlib.sha256(raw).hexdigest(),
             'classification':value.get('classification'),'locked_test_eligible':value.get('lockedTestEligible',False)}
        complete=directory/'COMPLETE.json'
        if not complete.exists():row['status']='INCOMPLETE_NO_SEAL';out.append(row);continue
        seal=json.loads(complete.read_text())
        if seal.get('manifestSha256')!=row['manifest_sha256']:
            row['status']='SEAL_MISMATCH';out.append(row);continue
        observations=directory/'observations.jsonl.gz'
        if not observations.exists():
            row.update(status='EXTENSION_OF_BASE_DATASET',base_manifest_sha256=value.get('baseManifestSha256'))
        else:
            # Full integrity hashing is deliberately not conflated with this
            # clock audit. Actual loaders verify row bytes before training.
            row.update(status='CLOCK_AUDITED',**clocks(observations))
        out.append(row)
    return {'classification':'all_local_compiled_dataset_clock_inventory_not_price_research',
        'utc':datetime.now(timezone.utc).isoformat(),'datasets':out,
        'duplicate_relocated_datasets_must_not_be_counted_as_more_history':True,
        'clock_coverage_is_not_complete_market_or_execution_calibration':True,
        'full_competition_history_verified':False,'prices_or_returns_exported':False,
        'production_changed':False}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--root',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);args=p.parse_args()
    result=audit(args.root)
    with args.output.open('x',encoding='utf8')as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps(result),flush=True)


if __name__=='__main__':main()
