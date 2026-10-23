"""Stage target-specific Idaho management; no reload, account client or order."""
from pathlib import Path
from datetime import datetime
import argparse
import base64
import copy
import hashlib
import importlib.util
import json
import os
import subprocess
import time


def sha(raw): return hashlib.sha256(raw).hexdigest()


def put(p,value):
    raw=(json.dumps(value,separators=(',',':'),ensure_ascii=False)+'\n').encode()
    with p.open('xb') as f: f.write(raw)
    return sha(raw)


def main(a):
    os.umask(0o077); os.nice(15); Path('/proc/self/oom_score_adj').write_text('1000')
    cfg=json.loads(a.config.read_bytes()); F=Path(cfg['outputRoot'])
    state_raw=(F/'state.json').read_bytes(); state=json.loads(state_raw)
    original=Path(state['controlPath']).read_bytes(); assert sha(original)==state['controlSha256']
    control=json.loads(original)
    reviews=[(pin,json.loads(Path(pin['path']).read_bytes())) for pin in control['fundamentalPolicy']['reviews']]
    assert not any(r['exchangeId']=='859' for _,r in reviews), 'TARGET_MANAGER_ALREADY_PRESENT'
    parent_pin,parent=next((pin,r) for pin,r in reviews if r['exchangeId']=='1051')
    assert parent.get('existingOrigin') is None
    a.output.mkdir(mode=0o700,parents=True,exist_ok=False)
    cfg=copy.deepcopy(cfg); cfg['exchanges']=sorted(set(cfg['exchanges'])|{'859'})
    supplemental=cfg['supplementalReference']; supplemental.update(exchangeIds=['1076','859'],
        seedPath=str(a.sources/'seed.json'),seedSha256=sha((a.sources/'seed.json').read_bytes()))
    script=Path(cfg['updaterScript']); assert sha(script.read_bytes())==cfg['updaterSha256']
    spec=importlib.util.spec_from_file_location('checked_source_capture',script)
    module=importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    capture=module.capture_values(cfg)
    target=next(row for row in capture['rows'] if row['exchangeId']=='859')
    put(a.output/'capture.original.json',capture)
    registry=json.loads((a.source/'originals/full-universe-expanded/census.private.json').read_bytes())['rows']
    row=next(r for r in registry if r['exchangeId']=='859')
    assert (row['raceName'],row['winnerName'])==('Governor of Idaho','Democratic Party')
    for k in ['marketId','contractId','officialContractRootSha256']: assert row[k]==target[k]
    root_path=a.source/'originals/roots'/row['originalFile']; root_raw=root_path.read_bytes(); root=json.loads(root_raw)
    assert sha(root_raw)==row['semanticSourceSha256'] and root['status']==200
    node=root['data']['root']; canonical=lambda v:json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
    assert sha(canonical(node))==row['officialContractRootSha256']
    clock=lambda s:int(datetime.fromisoformat(s.replace('Z','+00:00')).timestamp()*1000)
    settlement=clock(node['settlement_date'])
    probe=json.loads((a.sources/'source-probe.public.json').read_bytes()); ballot=a.sources/'ballot.original.pdf'
    assert sha(ballot.read_bytes())==probe['sha256'] and ballot.read_bytes().startswith(b'%PDF')
    assert probe['sourceUrl'].startswith('https://www.cassia.gov/media/Elections/')
    quote_raw=base64.b64decode(target['quoteRecordBase64'],validate=True)
    (a.output/'forecast.original.json').write_bytes(quote_raw)
    review=copy.deepcopy(parent); now=time.time_ns()//1000000
    review.update(reviewerId='Moloch-idaho-value-manager-015-20261023',exchangeId='859',
        marketId=row['marketId'],contractId=row['contractId'],officialContractRootSha256=row['officialContractRootSha256'],
        side='NO',marginPerShare='0.010000',maximumPrincipal='10000.000000',maximumQuantity=1000000,
        holdUntilMs=settlement,reviewedAtMs=now,expiresAtMs=now+900000,
        executionPolicy={'version':1,'allowedModes':['boundedLimitMayRest']})
    review['passiveValueExit']={'version':1,'maximumQuantity':10000}
    review['valuationObjective'].update(timingSourceSha256=sha(root_raw),scheduledSettlementAtMs=settlement)
    files=[]
    for i,source in enumerate(review['sources']):
        if source['kind']=='forecast':
            body=quote_raw; path=a.output/'forecast.original.json'; observed=target['quoteObservedAtMs']
            url='https://api.elections.kalshi.com/trade-api/v2/markets/'+target['kalshiTicker']+'/orderbook?depth=20'
        elif source['kind']=='official_ballot':
            body=ballot.read_bytes(); path=ballot; observed=clock(probe['receivedAt']); url=probe['sourceUrl']
        elif source['kind']=='official_contract_node':
            body=root_raw; path=root_path; observed=clock(root['receivedAt']); url='https://sig.thesuper.market/api/v1'+root['endpoint']
        else: raise RuntimeError('UNRECOGNIZED_TEMPLATE_SOURCE_KIND')
        review['sources'][i]=dict(source,url=url,sha256=sha(body),publishedAtMs=0,observedAtMs=observed)
        files.append(dict(path=str(path),sha256=sha(body)))
    template=a.output/'859-template.unapproved.json'; digest=put(template,review)
    control['fundamentalPolicy']['reviews'].append(dict(path=str(template),fileSha256=digest,sourceFiles=files))
    cp=a.output/'control-template.unapproved.json'; cp_sha=put(cp,control)
    modes=json.loads(Path(cfg['executionModesPolicy']).read_bytes()); modes['exchanges']['859']=['boundedLimitMayRest']
    modes.setdefault('entryMargins',{})['859']='0.010000'
    mp=a.output/'entry-policy.owner-selected.json'; mp_sha=put(mp,modes)
    assert sha(Path(cfg['stageScript']).read_bytes())==cfg['stageScriptSha256']
    subprocess.run(['python3',cfg['stageScript'],'--base-control',str(cp),'--base-sha256',cp_sha,
        '--capture',str(a.output/'capture.original.json'),'--output',str(a.output/'generation'),
        '--review-helper',cfg['reviewHelper'],'--execution-modes-policy',str(mp),
        '--execution-modes-sha256',mp_sha],check=True)
    assert (F/'state.json').read_bytes()==state_raw, 'SOURCE_GENERATION_CHANGED_DURING_STAGE'
    cfg.update(executionModesPolicy=str(mp),executionModesPolicySha256=mp_sha)
    put(a.output/'maintenance-config.unapproved.json',cfg)
    put(a.output/'decision.public.json',dict(exchangeId='859',side='NO',parentGeneration=state['generationSha256'],
        maximumPrincipal='10000.000000',maximumQuantity=1000000,physicalOrderMaximumUnchanged=True,
        captureScopes=[r['exchangeId'] for r in capture['rows']],sourceClocksRenewed=False,
        prospectiveFlatCohort=True,currentFlatAccountNotVerified=True,cohortInstalled=False,
        officialSigRequests=0,ordersSubmitted=0,productionChanged=False,
        next='Fresh account/cohort verification and authenticated existing-worker generation handoff required.'))
    print('UNAPPROVED_TARGET_MANAGER_STAGED',flush=True)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for n in ['config','sources','source','output']: p.add_argument('--'+n,type=Path,required=True)
    main(p.parse_args())
