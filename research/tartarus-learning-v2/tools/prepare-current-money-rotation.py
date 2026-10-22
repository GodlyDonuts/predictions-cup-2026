"""Prepare an unapproved first SELL of a positive conditional monetary rotation.

Recompute original FIFO, shared depth and every funding prefix. The later legs
remain hypothetical: they grant no orders, actual proceeds or future liquidity.
"""
import argparse
from hashlib import sha256
import importlib.util
import json
import os
from pathlib import Path
import sys


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def completion_ms(count, writes, gap):
    assert 1 <= count <= 128 and writes > 0
    writes, gap = min(24, writes), max(2500, gap)
    starts = [0]
    for _ in range(count):
        next_start = starts[-1]+gap
        if len(starts) >= writes:
            next_start = max(next_start, starts[-writes]+60000)
        starts.append(next_start)
    return starts[-1]


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
    reservation = load_module('rotation_reservation', Path(__file__).with_name('money-reservation-projection.py'))
    load = lambda p: json.loads(p.read_bytes())
    a.output.mkdir(mode=0o700, parents=True, exist_ok=False)
    originals = a.output/'originals'
    originals.mkdir(mode=0o700)
    compiled = load(a.inputs/'compiled-current.private.json')
    view_raw = (a.inputs/'native-view.original.json').read_bytes()
    view = json.loads(view_raw)
    model_raw = (a.inputs/'issued-reference-model.private.json').read_bytes()
    model = json.loads(model_raw)
    rows = load(a.source/'originals/full-universe-expanded/census.private.json')['rows']
    byex = {r['exchangeId']: r for r in rows}
    assert compiled['financial'] is not None and not compiled['unavailableReasons']
    assert compile_current((a.inputs/'report.original.json').read_bytes(), view_raw,
        expected_report_sha256=compiled['reportSha256'], expected_view_sha256=compiled['viewSha256'],
        registry=rows, conditional_future_legs=compiled['financial']['conditionalFutureLegs']) == compiled
    assert model['currentPriceInputsAtIssue'] and not model['datedResearchOverride']
    assert 0 < a.horizon_ms <= 900000 and model['issuedAtMs'] <= a.decision_ms
    until = model['inputCutoffMs']+a.horizon_ms
    view_clock = _ns(view['writtenAt'])//1000000
    assert view_clock <= a.decision_ms < min(until, view_clock+a.horizon_ms), 'ORIGINAL_REVIEW_HORIZON'
    plan_raw = a.plan.read_bytes()
    legs = json.loads(plan_raw)['legs']
    assert 2 <= len(legs) <= 129 and legs[0]['action'] == 'SELL', 'OWNED_SELL_ROTATION_REQUIRED'
    assert any(leg['action'] == 'BUY' for leg in legs[1:]), 'REPLACEMENT_BUY_REQUIRED'
    fin = compiled['financial']
    from research_book_observations import load as research_books
    books, book_sources = research_books(a.inputs, compiled, rows)
    checked = audit(fin, rows, books, model['model'], model['fields'], legs, assumptions=ASSUMPTIONS)
    interval = ordinary_money_bounds(rows, model['marginals'], model['profiles'], model['model']['diagnostics'], legs)
    floor = amount(a.minimum_expected_money)
    assert floor > 0 and all(m['expectedChangeMicrosNumerator'] >= floor*1000000 for m in checked['models']), 'ROTATION_DECLARED_MONEY_FLOOR'
    assert interval['ordinaryMinimumExpectedChangeMicros'] >= floor, 'ROTATION_ORDINARY_INTERVAL_FLOOR'
    first = legs[0]
    first_audit = audit(fin, rows, books, model['model'], model['fields'], [first], assumptions=ASSUMPTIONS)
    held = {byex[ex]['contractId'] for ex in fin['positions']}
    retained = held | {byex[leg['ex']]['contractId'] for leg in legs}
    projection = project(model['model']['scenarios'], model['model']['probabilityModels'], model['fields'],
        retained_contract_ids=sorted(retained), complete_held_contract_ids=sorted(held), maximum_native_scenarios=256)
    sources = {}

    def retain(raw, observed, age, label):
        assert observed <= a.decision_ms < observed+age, 'ORIGINAL_SOURCE_EXPIRED'
        h = sha256(raw).hexdigest()
        path = originals/(h+'.bin')
        if path.exists():
            assert path.read_bytes() == raw
        else:
            path.write_bytes(raw)
        sources[h] = dict(sha256=h, observedAtMs=observed, maximumAgeMs=age, assumptionLabel=label)
        return h

    retain(model_raw, model['inputCutoffMs'], a.horizon_ms,
        'Finite uncalibrated conditional price hypothesis; all underlying clocks retained. Not a renewed trading review or certified future quote.')
    book_source = retain(view_raw, view_clock, a.horizon_ms,
        'Original matched native book/account boundary. Later liquidity at these observed prices is conditional; each next physical action requires new original facts and approval.')
    book_source_by_exchange = {}
    for ex in {leg['ex'] for leg in legs}:
        if ex in book_sources:
            book_source_by_exchange[ex] = retain(book_sources[ex], _ns(books[ex]['receivedAt'])//1000000, a.horizon_ms,
                'Exact original SIG recorder row used for this conditional research leg. Original clocks remain; native admission, current execution and future liquidity are not asserted.')
    retain(plan_raw, view_clock, a.horizon_ms,
        'Retained proposed sequence; independent exact audit recomputed from original inputs. Not a fill, account fact or execution authority.')
    contracts = []
    for row in rows:
        if row['contractId'] not in retained:
            continue
        raw = (a.source/'originals/roots'/row['originalFile']).read_bytes()
        assert sha256(raw).hexdigest() == row['semanticSourceSha256']
        retain(raw, _ns(row['receivedAt'])//1000000, 7*86400000,
            'Retained official contract identity; target eligibility and exact identity require fresh physical verification.')
        contracts.append({k: row[k] for k in ('contractId', 'exchangeId', 'marketId', 'semanticSourceSha256', 'officialContractRootSha256')})
    money = lambda n: f'{n//1000000}.{n%1000000:06d}'
    ownership = {}
    for ex in {leg['ex'] for leg in legs}:
        proof = compiled['managedOwnership'].get(ex)
        if proof:
            retain(proof['sourceBodyText'].encode(), view_clock, a.horizon_ms,
                'Exact original cohort/FIFO ownership at capture, independently rechecked before each physical trade.')
            ownership[ex] = proof
    future = []
    for leg in legs[1:]:
        row = byex[leg['ex']]
        item = dict(contractId=row['contractId'], exchangeId=leg['ex'], marketId=row['marketId'],
            side=leg['side'], action=leg['action'], quantity=leg['quantity'], limitPrice=money(leg['price']),
            sourceSha256=book_source_by_exchange.get(leg['ex'], book_source),
            availableAtMs=_ns(books[leg['ex']]['receivedAt'])//1000000,
            validUntilMs=min(until, _ns(books[leg['ex']]['receivedAt'])//1000000+a.horizon_ms))
        if leg['ex'] in ownership:
            item['managedInventory'] = ownership[leg['ex']]
        future.append(item)
    pace = completion_ms(len(future), view['policy']['writesPerMinute'], view['policy']['writeGapMs'])
    assert a.decision_ms+pace < until, 'INSUFFICIENT_CONDITIONAL_COMPLETION_WINDOW'
    row = byex[first['ex']]
    projected = reservation.project(view, first['ex'])
    campaign = dict(schemaVersion=1, objective='declared_positive_expected_money_rotation',
        campaignStage='rotation_prerequisite', reviewedAtMs=a.decision_ms, validUntilMs=until,
        account=dict(positionsSha256=view['account']['positionsSha256'], cash=fin['cash'],
            reservedCash=projected['expectedReservedCash'], unsubmittedDraftYield=projected,
            actualFillsSha256=view['account']['actualFillsSha256'], modeledTerminalDebt='0.000000',
            liabilityAssumption='Conditional zero debt/no borrowing; independently reconcile actual collateral before dispatch.'),
        contracts=contracts, scenarios=projection['scenarios'], probabilityModels=projection['probabilityModels'],
        fieldModels=projection['fieldModels'], modelPairs=[dict(probabilityId=m['id'],fieldId=projection['fieldModels'][0]['id']) for m in projection['probabilityModels']],
        action=dict(contractId=row['contractId'], exchangeId=first['ex'], marketId=row['marketId'], side=first['side'], action='SELL',
            quantity=first['quantity'], limitPrice=money(first['price']), nominationBookSha256=sha256(canonical(books[first['ex']])).hexdigest()),
        risk=dict(maximumGrossCost='0.000000', maximumQuantity=first['quantity'],
            maximumModeledLoss=money(checked['maximumPrefixLossVersusHoldMicros']), minimumTerminalWealth='0.000000',
            minimumTargetGainPpm=0, minimumExpectedMoney=a.minimum_expected_money),
        contingentPlan=dict(assumptionLabel='Positive modeled full rotation; the owned first SELL may sacrifice expected value. Future orders can fail or disappear.',
            executionAuthority=False, actualFillAndFreshReapprovalRequired=True, minimumTargetGainPpm=0,
            executionAssumption=dict(minimumCompletionMs=pace, availableAtMs=view_clock, validUntilMs=until,
                conditionalFutureLiquidityAssumption='No projected cash is credited. After each zero/partial/full actual fill, hold actual cash/FIFO and obtain new books, residual economics and approval.'), tradeLegs=future),
        unmodeledStateStatement='Uncalibrated ordinary/exceptional scenarios. Full-plan expected change is conditional; first-leg loss, incomplete replacement, price moves and unavailable future depth are real risks. No present first-place/profit claim.')
    if first['ex'] in ownership:
        campaign['managedInventory'] = ownership[first['ex']]
    campaign['sources'] = list(sources.values())
    raw = canonical(campaign)+b'\n'
    assert len(raw) <= 2*1024*1024 and len(sources) <= 64
    (a.output/'nomination.json').write_bytes(raw)
    for name, value in [('exact-audit.private.json', checked), ('first-leg-audit.private.json', first_audit),
                        ('ordinary-interval-audit.private.json', interval)]:
        (a.output/name).write_text(json.dumps(value, indent=2)+'\n')
    result = dict(rawSha256=sha256(raw).hexdigest(), objective=campaign['objective'], exchangeId=first['ex'],
        side=first['side'], action='SELL', quantity=first['quantity'], limitPrice=money(first['price']), validUntilMs=until,
        weakestDeclaredExpectedChange=min(m['expectedChangeMicrosNumerator']/1e12 for m in checked['models']),
        firstLegExpectedChanges={m['id']:m['expectedChangeMicrosNumerator']/1e12 for m in first_audit['models']},
        ordinaryMinimumExpectedChange=interval['ordinaryMinimumExpectedChange'],
        monetaryFloorAppliesTo='full_conditional_rotation', conditionalFuturePhysicalLegs=len(future),
        minimumConditionalCompletionMs=pace, originalPlanSha256=sha256(plan_raw).hexdigest(),
        decisionProfilePolicy=model.get('decisionProfileSelection', {}).get('policy','all-declared'),
        decisionProfileIds=[m['id'] for m in projection['probabilityModels']], probabilityModels=len(projection['probabilityModels']),
        scenarios=len(projection['scenarios']), completeHeldMarkets=len(fin['positions']),
        actualFillPriceClaimed=False, futureLiquidityCertified=False, actualFutureProceedsCredited=False,
        sourceClocksRenewed=False, approvalPublished=False, submitted=False, executionAuthority=False)
    (a.output/'prepared.public.json').write_text(json.dumps(result, indent=2)+'\n')
    return result


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('source','inputs','plan','output'):
        p.add_argument('--'+name, type=Path, required=True)
    p.add_argument('--decision-ms', type=int, required=True)
    p.add_argument('--horizon-ms', type=int, default=300000)
    p.add_argument('--minimum-expected-money', default='0.010000')
    print(json.dumps(run(p.parse_args())))
