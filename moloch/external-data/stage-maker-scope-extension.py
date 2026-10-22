"""Add explicitly selected flat maker scopes to an immutable review generation.

No account API, ledger writes, reload submission or runtime restart. The existing
owner must independently prove flatness, exact contract, account and book before
installation/dispatch. Public party prices are uncalibrated conditional forecasts.
"""
import argparse
import base64
from datetime import datetime
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import subprocess
import time


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return json.dumps(value, separators=(',', ':'), ensure_ascii=False).encode()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for key in ('base-control', 'capture', 'scopes', 'output', 'review-helper'):
        p.add_argument('--' + key, type=Path, required=True)
    p.add_argument('--base-sha256', required=True)
    a = p.parse_args()
    original = a.base_control.read_bytes()
    if sha(original) != a.base_sha256 or a.output.exists():
        raise ValueError('PINNED_BASE_AND_NEW_OUTPUT_REQUIRED')
    control = json.loads(original)
    capture_raw = a.capture.read_bytes()
    capture = json.loads(capture_raw)
    now = time.time_ns() // 1_000_000
    if capture['model'] != 'conditional_Kalshi_party_midpoint_v1' or not 0 <= now-capture['capturedAtMs'] <= 120_000:
        raise ValueError('FRESH_CAPTURE_REQUIRED')
    scopes = json.loads(a.scopes.read_bytes())['scopes']
    rows = {x['exchangeId']: x for x in capture['rows']}
    if len(rows) != len(capture['rows']) or set(rows) != {x['exchangeId'] for x in scopes} or len(scopes) != len(rows):
        raise ValueError('EXACT_UNIQUE_SCOPE_CAPTURE')
    old_reviews = control['fundamentalPolicy']['reviews']
    old_inputs = [json.loads(Path(x['path']).read_bytes()) for x in old_reviews]
    if set(rows) & {x['exchangeId'] for x in old_inputs}:
        raise ValueError('NEW_SCOPES_ONLY')
    a.output.mkdir(parents=True)
    (a.output/'capture.original.json').write_bytes(capture_raw)
    reviews = list(old_reviews)
    trust = []
    for old in old_reviews:
        if sha(Path(old['path']).read_bytes()) != old['fileSha256']:
            raise ValueError('ORIGINAL_REVIEW_CHANGED')
        trust.append(subprocess.check_output([str(a.review_helper), 'hash-review', old['path']], text=True).strip())
    summaries = []
    for scope in scopes:
        ex = scope['exchangeId']; entry = rows[ex]
        node_raw = Path(scope['originalNodePath']).read_bytes()
        node = json.loads(node_raw)
        root = node['data']['root']
        canonical_root = json.dumps(root, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()
        if (sha(canonical_root) != entry['officialContractRootSha256'] or str(node['data']['market_id']) != entry['marketId']
                or str(root['contract_id']) != entry['contractId'] or node['status'] != 200):
            raise ValueError('ORIGINAL_CONTRACT_BINDING')
        ballot_path = Path(scope['ballotReviewPath'])
        ballot_binding = next((x for x in old_reviews if Path(x['path']) == ballot_path), None)
        if ballot_binding is None:
            raise ValueError('EXISTING_BALLOT_REVIEW_REQUIRED')
        ballot_review = json.loads(ballot_path.read_bytes())
        ballot_source = next(x for x in ballot_review['sources'] if x['kind'] == 'official_ballot')
        ballot_file = next(x for x in ballot_binding['sourceFiles'] if x['sha256'] == ballot_source['sha256'])
        if sha(Path(ballot_file['path']).read_bytes()) != ballot_source['sha256']:
            raise ValueError('ORIGINAL_BALLOT_CHANGED')
        # The execution owner selects same-state, genuinely covered ballot
        # scopes. Never silently repurpose a county's unrelated House ballot.
        if root['contract_details']['usState'] != scope['ballotState'] or not scope['ballotCoverageReviewed']:
            raise ValueError('EXPLICIT_SAME_STATE_BALLOT_COVERAGE')
        qraw = base64.b64decode(entry['quoteRecordBase64'], validate=True)
        q = json.loads(qraw)
        if (q['status'] != 200 or q['ticker'] != entry['kalshiTicker'] or sha(q['bodyText'].encode()) != q['bodySha256']
                or json.loads(q['bodyText']) != q['body'] or not 0 <= now-entry['quoteObservedAtMs'] <= 120_000):
            raise ValueError('FRESH_ORIGINAL_PUBLIC_QUOTE')
        book = q['body']['orderbook_fp']
        bid = max(Decimal(x[0]) for x in book['yes_dollars'])
        ask = 1-max(Decimal(x[0]) for x in book['no_dollars'])
        if not 0 < bid <= ask < 1 or (bid+ask)/2 != Decimal(entry['yesMidpoint']):
            raise ValueError('PUBLIC_MIDPOINT_BINDING')
        side = scope['side']
        if side not in ('YES', 'NO'):
            raise ValueError('BINARY_SIDE')
        point = (bid+ask)/2 if side == 'YES' else 1-(bid+ask)/2
        forecast = dict(version=1, kind='execution_owner_conditional_live_reference_model',
            captureSha256=sha(capture_raw), model=capture['model'], side=side,
            ordinaryProbability=format(point, '.6f'), calibratedProbabilityClaimed=False,
            settlementEquivalenceClaimed=False, mlFitOrSelectionUse=False, sourceEvidence=entry)
        fp = a.output/(ex+'-forecast.json'); fp.write_bytes(encoded(forecast))
        np = a.output/(ex+'-node.original.json'); np.write_bytes(node_raw)
        node_observed = int(datetime.fromisoformat(node['receivedAt'].replace('Z', '+00:00')).timestamp()*1000)
        scheduled = int(datetime.fromisoformat(root['settlement_date'].replace('Z', '+00:00')).timestamp()*1000)
        template = old_inputs[0]
        r = {k: template[k] for k in ('runId','bankManifestSha256','tournamentId')}
        r.update(version=2, reviewerId='Viserion-flat-maker-20261022',
            assumptionLabel='Owner-selected flat-inventory maker experiment. Original same-party Kalshi midpoint is an uncalibrated conditional payout assumption, not settlement equivalence. Exceptional probability allowance is 1 percent. Join-bid entries only; passive exits improve the observed ask where original FIFO permits at least .005 per share net. No ordinary 60-second forced liquidation. Actual settlement may be delayed; no guaranteed fill, turnover or ROI. Original official ballot and contract observation clocks retained. Fresh exact account/contract/depth admission belongs to the sole worker.',
            marketId=entry['marketId'], exchangeId=ex, contractId=entry['contractId'],
            officialContractRootSha256=entry['officialContractRootSha256'], side=side,
            ordinaryProbability=format(point, '.6f'), exceptionalProbabilityAllowance='0.010000',
            minimumExceptionalPayout='0.000000', marginPerShare='0.005000',
            maximumPrincipal=scope['maximumPrincipal'], maximumQuantity=10000,
            reviewedAtMs=now, expiresAtMs=entry['quoteObservedAtMs']+900_000,
            holdUntilMs=scheduled, payoutMappingIsOperatorAssumption=True, revokeExisting=False,
            passiveValueExit=dict(version=1, maximumQuantity=10000,
                pricingObjective=dict(kind='fifoProfitTurnover', minimumNetGain='0.005000')),
            sameSideTopUps=dict(version=3, maximumEntries=16),
            executionPolicy=dict(version=1, allowedModes=['joinBid']),
            valuationObjective=dict(kind='terminalSettlement', timingSourceSha256=sha(node_raw),
                timingJsonPointer='/data/root/settlement_date', scheduledSettlementAtMs=scheduled,
                delayPolicy='holdForActualSettlement'),
            sources=[dict(kind='forecast',url='https://api.elections.kalshi.com'+q['path'],sha256=sha(fp.read_bytes()),publishedAtMs=0,observedAtMs=entry['quoteObservedAtMs'],maximumAgeMs=900_000),
                dict(ballot_source,maximumAgeMs=14*86_400_000),
                dict(kind='official_contract_node',url='https://sig.thesuper.market/api/v1'+node['endpoint'],sha256=sha(node_raw),publishedAtMs=0,observedAtMs=node_observed,maximumAgeMs=14*86_400_000)])
        rp = a.output/(ex+'-review.json'); rp.write_bytes(encoded(r))
        canonical = subprocess.check_output([str(a.review_helper),'hash-review',str(rp)],text=True).strip()
        trust.append(canonical)
        reviews.append(dict(path=str(rp),fileSha256=sha(rp.read_bytes()),sourceFiles=[
            dict(path=str(fp),sha256=sha(fp.read_bytes())),ballot_file,dict(path=str(np),sha256=sha(node_raw))]))
        summaries.append(dict(exchangeId=ex,side=side,maximumPrincipal=scope['maximumPrincipal'],reviewSha256=canonical))
    ap = a.output/'authority.owner-reviewed.json'; ap.write_bytes(encoded(dict(version=1,trustedReviewSha256=sorted(trust))))
    control['fundamentalPolicy'] = dict(version=1,allowInitialization=False,authorityPath=str(ap),authoritySha256=sha(ap.read_bytes()),reviews=reviews)
    cp = a.output/'control.owner-reviewed.json'; cp.write_bytes(encoded(control))
    validation = json.loads(subprocess.check_output([str(a.review_helper),'validate-control',str(cp),str(time.time_ns()//1_000_000)],text=True))
    if not all(x['current'] for x in validation['reviews'] if x['exchangeId'] in rows):
        raise ValueError('NEW_REVIEW_NOT_CURRENT')
    if a.base_control.read_bytes() != original:
        raise ValueError('BASE_CHANGED')
    staged = dict(baseSha256=a.base_sha256,controlPath=str(cp),controlSha256=sha(cp.read_bytes()),changes=summaries,
        validation=validation,stagedOnly=True,officialSigRequests=0,reloadSent=False)
    (a.output/'staged.json').write_bytes(encoded(staged))
    print(json.dumps(dict(stagedOnly=True,newScopes=len(summaries),controlSha256=staged['controlSha256'])))


if __name__ == '__main__':
    main()
