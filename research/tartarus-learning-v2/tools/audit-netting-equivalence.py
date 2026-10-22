"""Audit gross exit equivalence on retained unified SIG books; never trade.

The complementary route consumes the SAME book levels. Its proceeds are not
additional liquidity or collateral, and cash admission/fees remain separate.
"""
import argparse
import base64
from datetime import datetime, timezone
from decimal import Decimal
from hashlib import sha256
import json
from pathlib import Path
from research_book_observations import row_books

SCALE = 1_000_000


def units(value):
    number = Decimal(str(value)) * SCALE
    assert number.is_finite() and number == number.to_integral_value()
    return int(number)


def money(value):
    return format(Decimal(value) / SCALE, '.6f')


def audit(inputs):
    book_raw = (inputs / 'research-book-observations.original.json').read_bytes()
    view_raw = (inputs / 'native-view.original.json').read_bytes()
    observations = json.loads(book_raw)
    view = json.loads(view_raw)
    assert observations['nativeViewSha256'] == sha256(view_raw).hexdigest()
    mapping = {ex: item['book']['marketId'] for ex, item in observations['observations'].items()}
    verified = {}
    for ex, item in observations['observations'].items():
        source = item['originalRowSha256']
        if source not in verified:
            raw = base64.b64decode(observations['originalRows'][source], validate=True)
            assert sha256(raw).hexdigest() == source
            verified[source] = row_books(raw, mapping, observations['tournamentId'], observations['capturedAtMs'])
        assert verified[source].get(ex) == item['book'], 'ORIGINAL_BOOK_MISMATCH'
    rows, missing = [], []
    for exchange, positions in sorted(view['account']['positions'].items()):
        for side, lots in positions.items():
            held = sum(lot['quantity'] for lot in lots)
            if not held:
                continue
            observation = observations['observations'].get(exchange)
            if observation is None:
                missing.append(dict(exchangeId=exchange, side=side, heldQuantity=held))
                continue
            book = observation['book']
            assert book['exchangeId'] == exchange
            # YES exits hit YES bids; NO exits hit complemented YES asks.
            levels = book['bids' if side == 'YES' else 'asks']
            left = min(held, 10000)
            target = left
            proceeds = complement_cost = count = 0
            for level in levels:
                price = units(level['price'])
                assert 0 < price < SCALE
                quantity = level['quantity']
                assert type(quantity) is int and quantity > 0
                take = min(left, quantity)
                exit_price = price if side == 'YES' else SCALE - price
                proceeds += take * exit_price
                complement_cost += take * (SCALE - exit_price)
                count += 1
                left -= take
                if not left:
                    break
            filled = target - left
            netting_cash_change = filled * SCALE - complement_cost
            assert netting_cash_change == proceeds
            rows.append(dict(exchangeId=exchange, side=side, heldQuantity=held,
                comparedQuantity=filled, requestedQuantity=target, levelsConsumed=count,
                directGrossProceeds=money(proceeds), oppositeBuyGrossCost=money(complement_cost),
                pairedPayout=money(filled * SCALE), nettingGrossCashChange=money(netting_cash_change),
                additionalGrossProceeds=money(netting_cash_change - proceeds),
                fullRequestedDepthAvailable=left == 0, sourceAt=book['sourceAt'],
                receivedAt=book['receivedAt'], originalRowSha256=observation['originalRowSha256']))
    return dict(schemaVersion=1, observedAt=datetime.now(timezone.utc).isoformat(),
        capturedAtMs=observations['capturedAtMs'], nativeViewWrittenAt=view['writtenAt'],
        sourceSha256=dict(books=sha256(book_raw).hexdigest(), nativeView=sha256(view_raw).hexdigest()),
        rows=rows, holdingsWithoutCapturedBook=missing,
        additionalGrossProceedsAcrossComparedHoldings=money(0),
        sameLiquidityConsumed=True, independentlyExecutableRoutesClaimed=False,
        originalRecorderRowsAndBooksVerified=True,
        comparisonExcludesFeesAndCashAdmission=True, currentExecutableLiquidityClaimed=False,
        accountRequests=0, ordersSubmitted=0, executionAuthority=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inputs', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    result = audit(args.inputs)
    with args.output.open('x', encoding='utf-8') as stream:
        json.dump(result, stream, indent=2)
        stream.write('\n')
    print(json.dumps(dict(comparedHoldings=len(result['rows']),
        missingBooks=len(result['holdingsWithoutCapturedBook']),
        additionalGrossProceeds=result['additionalGrossProceedsAcrossComparedHoldings'])))
