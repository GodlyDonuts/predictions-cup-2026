"""Stage explicit owner execution choices without renewing model/source clocks.

No account client or reload submission. Current forecast, objective, quantities,
principal, FIFO exit, source files and expiration remain byte/value-identical.
The existing worker independently rejects an open or otherwise ineligible target.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def put(path, value):
    with path.open('x', encoding='utf-8') as f:
        json.dump(value, f, separators=(',', ':'), ensure_ascii=False)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for key in ('base-control', 'output', 'review-helper'):
        p.add_argument('--'+key, type=Path, required=True)
    p.add_argument('--base-sha256', required=True)
    p.add_argument('--exchanges', required=True)
    p.add_argument('--modes', required=True)
    a = p.parse_args()
    selected = a.exchanges.split(','); modes = a.modes.split(',')
    if (len(set(selected)) != len(selected) or not selected or len(set(modes)) != len(modes)
            or not modes or not set(modes) <= {'joinBid','improveOneTick','boundedLimitMayRest'}):
        raise ValueError('EXPLICIT_UNIQUE_SCOPES_AND_MODES_REQUIRED')
    original = a.base_control.read_bytes()
    if sha(a.base_control) != a.base_sha256 or a.output.exists():
        raise ValueError('PINNED_BASE_AND_NEW_OUTPUT_REQUIRED')
    control = json.loads(original); now = time.time_ns()//1_000_000
    a.output.mkdir(parents=True); reviews=[]; trust=[]; changes=[]
    for old in control['fundamentalPolicy']['reviews']:
        if sha(old['path']) != old['fileSha256'] or any(sha(s['path']) != s['sha256'] for s in old['sourceFiles']):
            raise ValueError('ORIGINAL_REVIEW_OR_SOURCE_CHANGED')
        r = json.loads(Path(old['path']).read_bytes()); ex = r['exchangeId']
        if ex in selected:
            if r['version'] != 2 or r.get('existingOrigin') is not None or r['expiresAtMs'] <= now+60_000:
                raise ValueError('CURRENT_VERSIONED_SCOPE_REQUIRED')
            previous = r.get('executionPolicy')
            r['executionPolicy'] = dict(version=1, allowedModes=modes)
            # Keep all economic and clock fields, including review time, exact.
            # The new canonical owner hash authorizes only these mode choices.
            path = a.output/(ex+'-review.owner-modes.json'); put(path,r)
            current = dict(old,path=str(path),fileSha256=sha(path))
            changes.append(dict(exchangeId=ex,before=previous,after=r['executionPolicy'],
                originalExpiresAtMs=r['expiresAtMs'],maximumPrincipal=r['maximumPrincipal'],
                maximumQuantity=r['maximumQuantity']))
        else:
            current = old
        canonical = subprocess.check_output([str(a.review_helper),'hash-review',current['path']],text=True).strip()
        trust.append(canonical); reviews.append(current)
    if {x['exchangeId'] for x in changes} != set(selected):
        raise ValueError('EXACT_REVIEW_SCOPE_REQUIRED')
    ap=a.output/'authority.owner-reviewed.json';put(ap,dict(version=1,trustedReviewSha256=sorted(trust)))
    control['fundamentalPolicy']=dict(version=1,allowInitialization=False,authorityPath=str(ap),authoritySha256=sha(ap),reviews=reviews)
    cp=a.output/'control.owner-reviewed.json';put(cp,control)
    validation=json.loads(subprocess.check_output([str(a.review_helper),'validate-control',str(cp),str(time.time_ns()//1_000_000)],text=True))
    if not all(x['current'] for x in validation['reviews'] if x['exchangeId'] in selected):
        raise ValueError('SELECTED_REVIEW_NOT_CURRENT')
    if a.base_control.read_bytes()!=original:
        raise ValueError('BASE_CHANGED')
    staged=dict(baseSha256=a.base_sha256,controlPath=str(cp),controlSha256=sha(cp),changes=changes,
        validation=validation,stagedOnly=True,officialSigRequests=0,reloadSent=False,
        originalEconomicFieldsAndClocksPreserved=True)
    put(a.output/'staged.json',staged)
    print(json.dumps(dict(stagedOnly=True,changed=len(changes),controlSha256=staged['controlSha256'])))


if __name__ == '__main__':
    main()
