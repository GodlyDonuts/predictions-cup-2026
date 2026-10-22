"""Prepare one audited monetary nomination; no approval, network or live writes.

Requires a matched portfolio, issued reference matrix and a named first-order
candidate. Hypothetical later orders never finance this action. Original
source clocks survive; the explicit assumption horizon is not quote freshness.
"""
import argparse
from hashlib import sha256
import json
import os
from pathlib import Path
import sys


# Load the checked sibling without changing the public research module path.
import importlib.util
_projection_spec = importlib.util.spec_from_file_location('money_reservation_projection', Path(__file__).with_name('money-reservation-projection.py'))
_projection_module = importlib.util.module_from_spec(_projection_spec)
_projection_spec.loader.exec_module(_projection_module)
reservation_projection = _projection_module.project


def run(a):
    os.umask(0o077)
    sys.path.insert(0, str(a.source))
    from moloch.tartarus.sequence_audit import amount, audit
    from moloch.tartarus.sequential_frontier import ASSUMPTIONS
    from moloch.tartarus.scenario_projection import project
    from moloch.tartarus.categorical_money_bound import ordinary_money_bounds
    from moloch.tartarus.native_snapshot import _ns
    from moloch.tartarus.party_scenarios import canonical
    from moloch.tartarus.current_portfolio import compile_current
    a.output.mkdir(mode=0o700, parents=True, exist_ok=False)
    originals = a.output / 'originals'
    originals.mkdir(mode=0o700)
    load = lambda p: json.loads(p.read_bytes())
    compiled = load(a.inputs / 'compiled-current.private.json')
    view = load(a.inputs / 'native-view.original.json')
    model_raw = (a.inputs / 'issued-reference-model.private.json').read_bytes()
    model = json.loads(model_raw)
    rows = load(a.source / 'originals/full-universe-expanded/census.private.json')['rows']
    byex = {r['exchangeId']: r for r in rows}
    assert compiled['financial'] is not None and not compiled['unavailableReasons']
    assert compiled['viewSha256'] == sha256((a.inputs / 'native-view.original.json').read_bytes()).hexdigest()
    report_raw = (a.inputs / 'report.original.json').read_bytes()
    rebuilt = compile_current(report_raw, (a.inputs / 'native-view.original.json').read_bytes(),
        expected_report_sha256=compiled['reportSha256'], expected_view_sha256=compiled['viewSha256'],
        registry=rows, conditional_future_legs=compiled['financial']['conditionalFutureLegs'])
    assert rebuilt == compiled, 'DERIVED_PORTFOLIO_CHANGED_FROM_MATCHED_ORIGINALS'
    assert model['currentPriceInputsAtIssue'] and not model['datedResearchOverride']
    assert 0 < a.horizon_ms <= 900000 and model['issuedAtMs'] <= a.decision_ms
    until = model['inputCutoffMs'] + a.horizon_ms
    assert a.decision_ms < until, 'DECLARED_MODEL_HORIZON_EXPIRED'
    assert _ns(view['writtenAt']) // 1000000 <= a.decision_ms < _ns(view['writtenAt']) // 1000000 + a.horizon_ms, 'PORTFOLIO_REVIEW_HORIZON'
    fin = compiled['financial']
    candidates = load(a.candidates)['candidates']
    chosen = next(c for c in candidates if all(c['leg'][k] == v for k, v in
        [('ex', a.exchange), ('side', a.side), ('action', a.action)]))
    leg = dict(chosen['leg'])
    books = {b['exchangeId']: b for b in compiled['originalBooks']}
    assert a.exchange in books, 'ORIGINAL_CURRENT_NATIVE_TARGET_BOOK_REQUIRED'
    assert amount(a.minimum_expected_money) > 0
    tolerance = amount(getattr(a, 'price_tolerance', '0.000000'))
    assert 0 <= tolerance < 1000000, 'EXPLICIT_BINARY_PRICE_TOLERANCE'
    original_leg = dict(leg)
    # Use observed valid levels, not an invented tick size. The limit is a
    # worst-case funding/valuation price, never a claim about the actual fill.
    key = ('asks' if a.side == 'YES' else 'bids') if a.action == 'BUY' else ('bids' if a.side == 'YES' else 'asks')
    prices = {amount(level['price']) if a.side == 'YES' else 1000000 - amount(level['price'])
              for level in books[a.exchange][key]}
    prices.add(leg['price'])
    prices = sorted((p for p in prices if 0 < p < 1000000 and
        (leg['price'] <= p <= leg['price'] + tolerance if a.action == 'BUY'
         else leg['price'] - tolerance <= p <= leg['price'])), reverse=a.action == 'BUY')
    for price in prices:
        proposed = dict(original_leg, price=price)
        if a.action == 'BUY':
            market = sum(lot['quantity'] * amount(lot['price']) for lots in fin['positions'].get(a.exchange, {}).values() for lot in lots)
            proposed['quantity'] = min(original_leg['quantity'],
                (amount(fin['cash']) - amount(fin['reservedCash'])) // price,
                (amount(fin['maximumCommitted']) - amount(fin['ownedCost']) - amount(fin['reservedCash'])) // price,
                (amount(fin['maximumMarketCost']) - market) // price)
        if proposed['quantity'] <= 0:
            continue
        try:
            checked = audit(fin, rows, books, model['model'], model['fields'], [proposed], assumptions=ASSUMPTIONS)
            interval = ordinary_money_bounds(rows, model['marginals'], model['profiles'], model['model']['diagnostics'], [proposed])
        except ValueError:
            continue
        if (all(m['expectedChangeMicrosNumerator'] >= amount(a.minimum_expected_money) * 1000000 for m in checked['models'])
                and interval['ordinaryMinimumExpectedChangeMicros'] >= amount(a.minimum_expected_money)):
            leg = proposed
            break
    else:
        raise ValueError('NO_INDEPENDENTLY_POSITIVE_FUNDED_PRICE_WITHIN_REVIEW')
    held = {byex[ex]['contractId'] for ex in fin['positions']}
    retained = held | {byex[a.exchange]['contractId']}
    projection = project(model['model']['scenarios'], model['model']['probabilityModels'], model['fields'],
        retained_contract_ids=sorted(retained), complete_held_contract_ids=sorted(held), maximum_native_scenarios=256)
    sources = {}

    def retain(raw, observed, maximum_age, label):
        h = sha256(raw).hexdigest()
        assert observed <= a.decision_ms < observed + maximum_age, 'ORIGINAL_SOURCE_EXPIRED'
        (originals / (h + '.bin')).write_bytes(raw)
        sources[h] = dict(sha256=h, observedAtMs=observed, maximumAgeMs=maximum_age, assumptionLabel=label)
        return h

    retain(model_raw, model['inputCutoffMs'], a.horizon_ms,
        'Explicit finite-horizon uncalibrated price/stress hypothesis. Individual source clocks and expired opinions remain in the original. This horizon does not claim a fresh underlying quote or a renewed original trading review.')
    contracts = []
    for row in rows:
        if row['contractId'] not in retained:
            continue
        raw = (a.source / 'originals/roots' / row['originalFile']).read_bytes()
        assert sha256(raw).hexdigest() == row['semanticSourceSha256']
        retain(raw, _ns(row['receivedAt']) // 1000000, 7 * 86400000,
            'Retained original official contract identity. Current target identity and eligibility are independently checked before dispatch; no old book is made fresh.')
        contracts.append({k: row[k] for k in ('contractId', 'exchangeId', 'marketId', 'semanticSourceSha256', 'officialContractRootSha256')})
    n, p = leg['quantity'], leg['price']
    money = lambda micros: f'{micros // 1000000}.{micros % 1000000:06d}'
    row = byex[a.exchange]
    projection_after_yield = reservation_projection(view, a.exchange)
    campaign = dict(schemaVersion=1, objective='declared_positive_expected_money', campaignStage='direct',
        reviewedAtMs=a.decision_ms, validUntilMs=until, sources=None,
        account=dict(positionsSha256=view['account']['positionsSha256'], cash=fin['cash'], reservedCash=projection_after_yield['expectedReservedCash'],
                     unsubmittedDraftYield=projection_after_yield,
                     actualFillsSha256=view['account']['actualFillsSha256'], modeledTerminalDebt='0.000000',
                     liabilityAssumption='Conditional zero outstanding debt, no future borrowing; actual collateral must independently confirm coverage before dispatch.'),
        contracts=contracts, scenarios=projection['scenarios'], probabilityModels=projection['probabilityModels'],
        fieldModels=projection['fieldModels'],
        modelPairs=[dict(probabilityId=m['id'], fieldId=projection['fieldModels'][0]['id']) for m in projection['probabilityModels']],
        action=dict(contractId=row['contractId'], exchangeId=a.exchange, marketId=row['marketId'], side=a.side,
                    action=a.action, quantity=n, limitPrice=money(p), nominationBookSha256=sha256(canonical(books[a.exchange])).hexdigest()),
        risk=dict(maximumGrossCost=money(n * p if a.action == 'BUY' else 0), maximumQuantity=n,
                  maximumModeledLoss=money(checked['maximumPrefixLossVersusHoldMicros']), minimumTerminalWealth='0.000000',
                  minimumTargetGainPpm=0, minimumExpectedMoney=a.minimum_expected_money),
        unmodeledStateStatement='Uncalibrated sampled ordinary/exceptional assumptions; exact ordinary interval audit is separate. Source publication times, resolver differences, uncaptured exceptions, rival actions and future liquidity remain uncertain. Positive expected money does not assert realized profit or immediate first-place improvement.')
    ownership = compiled['managedOwnership'].get(a.exchange)
    if ownership:
        body = ownership['sourceBodyText'].encode()
        retain(body, _ns(view['writtenAt']) // 1000000, a.horizon_ms,
            'Original exact cohort ownership at the matched native boundary; current generation and FIFO ownership must be revalidated physically.')
        campaign['managedInventory'] = ownership
    campaign['sources'] = list(sources.values())
    raw = canonical(campaign) + b'\n'
    assert len(raw) <= 2 * 1024 * 1024 and len(sources) <= 64
    (a.output / 'nomination.json').write_bytes(raw)
    (a.output / 'exact-audit.private.json').write_text(json.dumps(checked, indent=2) + '\n')
    (a.output / 'ordinary-interval-audit.private.json').write_text(json.dumps(interval, indent=2) + '\n')
    result = dict(rawSha256=sha256(raw).hexdigest(), objective=campaign['objective'], exchangeId=a.exchange,
        side=a.side, action=a.action, quantity=n, limitPrice=money(p), validUntilMs=until,
        weakestDeclaredExpectedChange=min(m['expectedChangeMicrosNumerator'] / 1e12 for m in checked['models']),
        ordinaryMinimumExpectedChange=interval['ordinaryMinimumExpectedChange'], scenarios=len(projection['scenarios']),
        originalHypothesisQuantity=original_leg['quantity'], originalHypothesisPrice=money(original_leg['price']),
        requestedPriceTolerance=getattr(a, 'price_tolerance', '0.000000'),
        limitIsConservativeFundingAndValuationPrice=True, actualFillPriceClaimed=False,
        probabilityModels=len(projection['probabilityModels']),
        decisionProfilePolicy=model.get('decisionProfileSelection', {}).get('policy', 'all-declared'),
        decisionProfileIds=[m['id'] for m in projection['probabilityModels']], completeHeldMarkets=len(fin['positions']),
        sourceClocksRenewed=False, approvalPublished=False, submitted=False, executionAuthority=False)
    (a.output / 'prepared.public.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('source', 'inputs', 'candidates', 'output'):
        p.add_argument('--' + name, type=Path, required=True)
    p.add_argument('--exchange', required=True)
    p.add_argument('--side', choices=('YES', 'NO'), required=True)
    p.add_argument('--action', choices=('BUY', 'SELL'), required=True)
    p.add_argument('--decision-ms', type=int, required=True)
    p.add_argument('--horizon-ms', type=int, required=True)
    p.add_argument('--minimum-expected-money', required=True)
    p.add_argument('--price-tolerance', default='0.000000')
    run(p.parse_args())
