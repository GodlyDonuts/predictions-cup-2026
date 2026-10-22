"""File-only two-order allocation screen; later sale proceeds are hypothetical.

Rank first FIFO sale tranches and first acquisition tranches with exact sampled
payoff arithmetic, then independently audit funding, shared depth, every FIFO
breakpoint and ordinary interval expectations. No account or owner-file writes.
This finite screen is not a claim of globally optimal allocation or calibrated
returns. Replacements always require actual fills and a new physical review.
"""
import argparse
from hashlib import sha256
import json
from pathlib import Path
import sys


def fifo_basis(lots, quantity, amount):
    left, basis = quantity, 0
    for lot in lots:
        take = min(left, lot['quantity'])
        left -= take
        basis += take*amount(lot['price'])
        if not left:
            break
    assert not left, 'SALE_EXCEEDS_FIFO'
    return basis


def minimum_funding_sale(lots, maximum, price, cash, reserved, owned,
                         maximum_committed, acquisition_cost, amount):
    def funded(n):
        return (cash-reserved+n*price >= acquisition_cost
                and maximum_committed-owned-reserved+fifo_basis(lots, n, amount) >= acquisition_cost)
    if not funded(maximum):
        return None
    lo, hi = 0, maximum
    while lo < hi:
        mid = (lo+hi)//2
        if funded(mid):
            hi = mid
        else:
            lo = mid+1
    return lo


def run(a):
    sys.path.insert(0, str(a.source))
    from moloch.tartarus.sequence_audit import amount, audit, UNIT
    from moloch.tartarus.sequential_frontier import _columns, ASSUMPTIONS
    from moloch.tartarus.categorical_money_bound import ordinary_money_bounds
    from research_book_observations import load as research_books
    read = lambda path: json.loads(path.read_bytes())
    compiled = read(a.inputs/'compiled-current.private.json')
    fin = compiled['financial']
    assert fin is not None and not compiled['unavailableReasons']
    model = read(a.inputs/'issued-reference-model.private.json')
    rows = read(a.source/'originals/full-universe-expanded/census.private.json')['rows']
    byex = {r['exchangeId']: r for r in rows}
    books, _ = research_books(a.inputs, compiled, rows)
    columns, _, _ = _columns(fin, rows, books)
    first = [c for c in columns if c.get('previous') is None]
    sales = [c for c in first if c['action'] == 'SELL']
    buys = [c for c in first if c['action'] == 'BUY' and c.get('opposite_sale') is None]
    cash, reserved = amount(fin['cash']), amount(fin['reservedCash'])
    owned = amount(fin['ownedCost'])
    market = {ex: sum(l['quantity']*amount(l['price']) for lots in sides.values() for l in lots)
              for ex, sides in fin['positions'].items()}
    masses = {}
    for ex in books:
        for side in ('YES', 'NO'):
            target = 'resolved_yes' if side == 'YES' else 'resolved_no'
            labels = [s['payouts'][byex[ex]['contractId']] for s in model['model']['scenarios']]
            masses[ex, side] = [(sum(w for w, label in zip(m['weightsPpm'], labels) if label == target),
                                sum(w for w, label in zip(m['weightsPpm'], labels) if label == 'refund_principal'))
                               for m in model['model']['probabilityModels']]
    proposals = []
    floor = amount(a.minimum_expected_money)*UNIT
    for sale in sales:
        sn, sp, basis = sale['n'], sale['price'], sale['basis']
        for buy in buys:
            if buy['ex'] == sale['ex']:
                continue
            bp = buy['price']
            n = min(buy['n'], (cash-reserved+sn*sp)//bp,
                    (amount(fin['maximumCommitted'])-owned-reserved+basis)//bp,
                    (amount(fin['maximumMarketCost'])-market.get(buy['ex'], 0))//bp)
            if n <= 0:
                continue
            lots = fin['positions'][sale['ex']][sale['side']]
            minimal = minimum_funding_sale(lots, sn, sp, cash, reserved, owned,
                amount(fin['maximumCommitted']), n*bp, amount)
            for sold in sorted({sn, minimal} - {None, 0}):
                sold_basis = fifo_basis(lots, sold, amount)
                sale_scores = [sold*sp*UNIT-sold*win*UNIT-sold_basis*refund
                               for win, refund in masses[sale['ex'], sale['side']]]
                scores = [s+n*(win*UNIT-(UNIT-refund)*bp)
                          for s, (win, refund) in zip(sale_scores, masses[buy['ex'], buy['side']])]
                if min(scores) < floor:
                    continue
                legs = [dict(sale, quantity=sold, basis=sold_basis), dict(buy, quantity=n)]
                proposals.append((min(scores), scores, legs))
    proposals.sort(key=lambda p: p[0], reverse=True)
    candidates, rejected, audited = [], {}, 0
    for _, scores, legs in proposals[:a.audit_limit]:
        audited += 1
        try:
            checked = audit(fin, rows, books, model['model'], model['fields'], legs, assumptions=ASSUMPTIONS)
            actual = [m['expectedChangeMicrosNumerator'] for m in checked['models']]
            assert actual == scores, 'INDEPENDENT_PAYOFF_ARITHMETIC_DISAGREES'
            interval = ordinary_money_bounds(rows, model['marginals'], model['profiles'],
                                             model['model']['diagnostics'], legs)
            if interval['ordinaryMinimumExpectedChangeMicros'] < amount(a.minimum_expected_money):
                continue
        except ValueError as error:
            rejected[str(error)] = rejected.get(str(error), 0)+1
            continue
        candidates.append(dict(legs=legs, weakestDeclaredExpectedChange=min(actual)/1e12,
            ordinaryMinimumExpectedChange=interval['ordinaryMinimumExpectedChange'],
            eventDescriptions=[dict(exchangeId=l['ex'], race=byex[l['ex']]['raceName'],
                                    party=byex[l['ex']]['winnerName']) for l in legs],
            maximumPrefixLossVersusHoldMicros=checked['maximumPrefixLossVersusHoldMicros'],
            minimumPrefixWealthMicros=checked['minimumPrefixWealthMicros'],
            hypotheticalGrossSaleProceedsMicros=legs[0]['quantity']*legs[0]['price'],
            hypotheticalAcquisitionCostMicros=legs[1]['quantity']*legs[1]['price'],
            actualFutureProceedsCredited=False, futureLiquidityCertified=False,
            independentExpectedPayoffArithmeticVerified=True, executionAuthority=False))
        if len(candidates) >= a.maximum_candidates:
            break
    result = dict(sourcePortfolioSha256=compiled['reportSha256'], sourceViewSha256=compiled['viewSha256'],
        originalViewWrittenAt=compiled['originalViewWrittenAt'],
        issuedReferenceSha256=sha256((a.inputs/'issued-reference-model.private.json').read_bytes()).hexdigest(),
        firstSaleTranches=len(sales), firstAcquisitionTranches=len(buys),
        positiveSampledProposals=len(proposals), independentlyAudited=audited,
        researchAuditLimit=a.audit_limit, researchMaximumOutputCandidates=a.maximum_candidates,
        candidates=candidates, rejections=rejected, globalOptimalityClaimed=False,
        calibratedProbabilitiesClaimed=False, newAccountClients=0, executionAuthority=False)
    with a.output.open('x') as f:
        json.dump(result, f, indent=2); f.write('\n')
    print(json.dumps(dict(candidates=len(candidates), proposals=len(proposals), audited=audited,
                         first=candidates[:1], executionAuthority=False)), flush=True)
    return result


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('source', 'inputs', 'output'):
        p.add_argument('--'+name, type=Path, required=True)
    p.add_argument('--minimum-expected-money', default='0.010000')
    p.add_argument('--audit-limit', type=int, default=256)
    p.add_argument('--maximum-candidates', type=int, default=10)
    a = p.parse_args()
    assert 1 <= a.audit_limit <= 10000 and 1 <= a.maximum_candidates <= a.audit_limit
    run(a)
