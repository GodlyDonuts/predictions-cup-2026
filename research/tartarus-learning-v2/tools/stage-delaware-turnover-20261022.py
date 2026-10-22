"""Stage one new, flat Delaware Republican-NO review; never send a reload.

Uses the existing conditional-reference model and recurring FIFO-profit exits.
All prior review bytes, monetary controls and original account history survive.
Actual same-worker account/contract/depth reconciliation remains mandatory.
"""
import argparse
import base64
import hashlib
import json
import subprocess
import time
from datetime import datetime
from decimal import Decimal
from pathlib import Path


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def clock(value):
    return int(datetime.fromisoformat(value.replace('Z', '+00:00')).timestamp() * 1000)


def write(path, value):
    raw = json.dumps(value, separators=(',', ':'), ensure_ascii=False).encode()
    with path.open('xb') as f:
        f.write(raw)
    return digest(raw)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for key in ('base-control', 'capture', 'ballot', 'ballot-receipt', 'contract', 'output', 'review-helper'):
        p.add_argument('--' + key, type=Path, required=True)
    p.add_argument('--base-sha256', required=True)
    a = p.parse_args()
    base_raw = a.base_control.read_bytes()
    assert digest(base_raw) == a.base_sha256 and not a.output.exists()
    control = json.loads(base_raw)
    original = control['fundamentalPolicy']['reviews']
    templates = []
    for old in original:
        raw = Path(old['path']).read_bytes()
        assert digest(raw) == old['fileSha256']
        review = json.loads(raw)
        assert review['exchangeId'] != '1075', 'New scope already installed'
        if review['exchangeId'] == '1077':
            templates.append(review)
    assert len(templates) == 1
    review = templates[0]
    assert review['version'] == 2 and review['side'] == 'NO'
    assert review['sameSideTopUps'] == {'version': 3, 'maximumEntries': 16}
    assert review['passiveValueExit']['pricingObjective'] == {
        'kind': 'fifoProfitTurnover', 'minimumNetGain': '0.005000'}
    assert review['maximumPrincipal'] == '10000.000000'
    assert review['exceptionalProbabilityAllowance'] == '0.010000'
    assert review['marginPerShare'] == '0.010000'
    assert review.get('existingOrigin') is None

    now = time.time_ns() // 1_000_000
    capture_raw = a.capture.read_bytes()
    capture = json.loads(capture_raw)
    assert capture['model'] == 'conditional_Kalshi_party_midpoint_v1'
    assert 0 <= now - capture['capturedAtMs'] <= 120000
    assert len(capture['rows']) == 1
    entry = capture['rows'][0]
    assert (entry['exchangeId'], entry['marketId'], entry['contractId'], entry['kalshiTicker']) == (
        '1075', '386', '387', 'SENATEDE-26-R')
    quote = json.loads(base64.b64decode(entry['quoteRecordBase64'], validate=True))
    assert quote['status'] == 200 and quote['ticker'] == entry['kalshiTicker']
    assert digest(quote['bodyText'].encode()) == quote['bodySha256']
    assert json.loads(quote['bodyText']) == quote['body']
    assert clock(quote['receivedAt']) == entry['quoteObservedAtMs']
    assert 0 <= now - entry['quoteObservedAtMs'] <= 120000
    book = quote['body']['orderbook_fp']
    bid = max(Decimal(x[0]) for x in book['yes_dollars'])
    ask = 1 - max(Decimal(x[0]) for x in book['no_dollars'])
    midpoint = (bid + ask) / 2
    assert 0 < bid <= ask < 1 and midpoint == Decimal(entry['yesMidpoint'])
    point = 1 - midpoint

    node_raw = a.contract.read_bytes()
    node = json.loads(node_raw)
    root = node['data']['root']
    assert node['status'] == 200 and node['data']['market_id'] == '386'
    assert root['contract_id'] == root['node_id'] == '387' and root['settled_with'] is None
    details = root['contract_details']
    assert details['raceName'] == 'U.S. Senate Delaware' and details['winnerName'] == 'Republican Party'
    assert details['electionDate'] == '2026-11-03' and details['resolutionType'] == 'Party Winner'
    root_sha = digest(json.dumps(root, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode())
    assert root_sha == entry['officialContractRootSha256']
    ballot = a.ballot.read_bytes()
    ballot_receipt = json.loads(a.ballot_receipt.read_bytes())
    assert ballot_receipt['status'] == 200 and digest(ballot) == ballot_receipt['bodySha256']
    assert ballot_receipt['url'] == 'https://elections.delaware.gov/elections/general/sampleballots/GE2026/02-27.html'
    assert all(word in ballot.upper() for word in [b'COONS', b'KATZ', b'U.S. SENATOR', b'REPUBLICAN'])
    assert 0 <= now - clock(ballot_receipt['receivedAt']) < 86400000

    a.output.mkdir(parents=True)
    (a.output / 'capture.original.json').write_bytes(capture_raw)
    (a.output / 'contract.original.json').write_bytes(node_raw)
    (a.output / 'ballot.original.html').write_bytes(ballot)
    write(a.output / 'ballot.receipt.json', ballot_receipt)
    forecast = dict(version=1, kind='execution_owner_conditional_live_reference_model',
        captureSha256=digest(capture_raw), capturedAtMs=capture['capturedAtMs'],
        model=capture['model'], side='NO', ordinaryProbability=format(point, '.6f'),
        calibratedProbabilityClaimed=False, settlementEquivalenceClaimed=False,
        mlFitOrSelectionUse=False, sourceEvidence=entry)
    fp = a.output / '1075-forecast.json'
    forecast_sha = write(fp, forecast)
    sources = [
        dict(kind='forecast', url='https://api.elections.kalshi.com'+quote['path'], sha256=forecast_sha,
             publishedAtMs=0, observedAtMs=entry['quoteObservedAtMs'], maximumAgeMs=900000),
        dict(kind='official_ballot', url=ballot_receipt['url'], sha256=digest(ballot),
             publishedAtMs=0, observedAtMs=clock(ballot_receipt['receivedAt']), maximumAgeMs=14*86400000),
        dict(kind='official_contract_node',
             url='https://sig.thesuper.market/api/v1/markets/386/nodes?tournamentId='+review['tournamentId'],
             sha256=digest(node_raw), publishedAtMs=0,
             observedAtMs=clock(node['receivedAt']), maximumAgeMs=14*86400000)]
    review.update(reviewerId='Viserion-Delaware-turnover-20261022', exchangeId='1075', marketId='386',
        contractId='387', officialContractRootSha256=root_sha,
        ordinaryProbability=format(point, '.6f'), reviewedAtMs=now,
        expiresAtMs=entry['quoteObservedAtMs']+900000,
        holdUntilMs=clock(root['settlement_date']), sources=sources,
        valuationObjective=dict(kind='terminalSettlement', timingSourceSha256=digest(node_raw),
            timingJsonPointer='/data/root/settlement_date', scheduledSettlementAtMs=clock(root['settlement_date']),
            delayPolicy='holdForActualSettlement'),
        assumptionLabel='Owner-selected conditional Kalshi Republican-party NO midpoint model for Delaware Senate. '
        'SIG party winner and Kalshi swearing-in are related but not identical settlement events. '
        'The 1% exceptional allowance and 1cent margin are operator assumptions, not calibrated bounds. '
        'Actual ballot includes Coons Democrat, Katz Republican and write-in; resolver, replacement, cancellation and delay uncertainty remain. '
        'New flat cohort only, within the original shared bank; 10000 maximum principal and existing physical quantity limit. '
        'Recurring acquisition and FIFO profit turnover minimum0.005/share follow the existing strategy. '
        'The scheduled settlement date is not promised liquidity; intraday convergence and one filled trade/minute are not established. '
        'No existing holdings or cost basis are adopted or reset. Current official contract, account and depth remain required before dispatch.')
    rp = a.output / '1075-review.json'
    raw_sha = write(rp, review)
    canonical = subprocess.check_output([str(a.review_helper), 'hash-review', str(rp)], text=True).strip()
    files = [dict(path=str(path), sha256=s['sha256']) for s, path in zip(sources,
        [fp, a.output/'ballot.original.html', a.output/'contract.original.json'])]
    trust = [subprocess.check_output([str(a.review_helper), 'hash-review', old['path']], text=True).strip()
             for old in original] + [canonical]
    authority = a.output / 'authority.owner-reviewed.json'
    authority_sha = write(authority, dict(version=1, trustedReviewSha256=sorted(trust)))
    control['fundamentalPolicy'] = dict(version=1, allowInitialization=False,
        authorityPath=str(authority), authoritySha256=authority_sha,
        reviews=original + [dict(path=str(rp), fileSha256=raw_sha, sourceFiles=files)])
    cp = a.output / 'control.owner-reviewed.json'
    control_sha = write(cp, control)
    checked = json.loads(subprocess.check_output([str(a.review_helper), 'validate-control',
        str(cp), str(time.time_ns() // 1_000_000)], text=True))
    assert next(r for r in checked['reviews'] if r['exchangeId']=='1075')['current']
    assert a.base_control.read_bytes() == base_raw
    write(a.output / 'staged.json', dict(baseSha256=a.base_sha256, controlPath=str(cp),
        controlSha256=control_sha, reviewSha256=canonical, validation=checked,
        preservedOriginalReviews=len(original), newExchange='1075', side='NO',
        conditionalOrdinaryProbability=str(point), stagedOnly=True, accountRequests=0,
        reloadSent=False, tradingWorkerRestarted=False))
    print(json.dumps(dict(controlPath=str(cp), controlSha256=control_sha,
        stagedOnly=True, reloadSent=False)))


if __name__ == '__main__':
    main()
