"""Finite first-order monetary screen over matched native books; no API/approval.

Each candidate must independently fit actual cash, original FIFO, market caps
and all declared probability models AND the exact ordinary interval bound.
"""
import argparse
import json
from pathlib import Path
import sys


def run(args):
    sys.path.insert(0, str(args.source))
    from moloch.tartarus.sequential_frontier import _columns, ASSUMPTIONS
    from moloch.tartarus.sequence_audit import audit, amount
    from moloch.tartarus.categorical_money_bound import ordinary_money_bounds
    load = lambda path: json.loads(path.read_bytes())
    compiled = load(args.inputs / 'compiled-current.private.json')
    financial = compiled['financial']
    assert financial is not None and not compiled['unavailableReasons']
    model = load(args.inputs / 'issued-reference-model.private.json')
    registry = load(args.source / 'originals/full-universe-expanded/census.private.json')['rows']
    books = {book['exchangeId']: book for book in compiled['originalBooks']}
    columns, _, _ = _columns(financial, registry, books)
    minimum = amount(args.minimum_expected_money)
    assert minimum > 0
    out, rejected, seen = [], {}, set()
    for column in columns:
        ex, side, price, n = (column[k] for k in ('ex', 'side', 'price', 'n'))
        if column['action'] == 'BUY':
            opposite = 'NO' if side == 'YES' else 'YES'
            if any(lot['quantity'] for lot in financial['positions'].get(ex, {}).get(opposite, [])):
                continue
            market = sum(lot['quantity'] * amount(lot['price'])
                         for lots in financial['positions'].get(ex, {}).values() for lot in lots)
            n = min(n, (amount(financial['cash']) - amount(financial['reservedCash'])) // price,
                (amount(financial['maximumCommitted']) - amount(financial['ownedCost']) - amount(financial['reservedCash'])) // price,
                (amount(financial['maximumMarketCost']) - market) // price)
        if n <= 0:
            continue
        key = (ex, side, column['action'], n, price)
        if key in seen:
            continue
        seen.add(key)
        leg = dict(column, quantity=n)
        try:
            checked = audit(financial, registry, books, model['model'], model['fields'], [leg], assumptions=ASSUMPTIONS)
            numerators = [m['expectedChangeMicrosNumerator'] for m in checked['models']]
            if min(numerators) < minimum * 1000000:
                continue
            interval = ordinary_money_bounds(registry, model['marginals'], model['profiles'],
                model['model']['diagnostics'], [leg])
            if interval['ordinaryMinimumExpectedChangeMicros'] < minimum:
                continue
        except ValueError as error:
            reason = str(error)
            rejected[reason] = rejected.get(reason, 0) + 1
            continue
        out.append(dict(leg=leg, weakestDeclaredExpectedChange=min(numerators) / 1e12,
            expectedChangeMicrosNumerators=numerators, bookReceivedAt=books[ex]['receivedAt'],
            exactOrdinaryIntervalSensitivity=interval,
            maximumPrefixLossVersusHoldMicros=checked['maximumPrefixLossVersusHoldMicros'],
            minimumPrefixWealthMicros=checked['minimumPrefixWealthMicros'], executionAuthority=False))
    out.sort(key=lambda row: row['weakestDeclaredExpectedChange'], reverse=True)
    result = dict(candidateCount=len(out), candidates=out, rejections=rejected,
        sourcePortfolioSha256=compiled['reportSha256'], originalViewWrittenAt=compiled['originalViewWrittenAt'],
        currentExecutableLiquidityClaimed=False, calibratedProbabilityClaimed=False,
        newAccountClients=0, executionAuthority=False)
    with args.output.open('x', encoding='utf-8') as stream:
        stream.write(json.dumps(result, indent=2) + '\n')
    print(json.dumps(dict(candidateCount=len(out), first=out[:1], executionAuthority=False)))
    return result


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source', type=Path, required=True)
    p.add_argument('--inputs', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--minimum-expected-money', required=True)
    run(p.parse_args())
