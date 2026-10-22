"""File-only SIG book observations for research, separate from native admission.

Retain exact original recorder rows. A snapshot can nominate a research price
without asserting complete trade history, an atomic account snapshot or physical
permission. The existing worker still recovers source and reads its final facts.
"""
import base64
from datetime import datetime
from decimal import Decimal
from hashlib import sha256
import json
from pathlib import Path

# Research prices nominate a hypothesis, never a physical dispatch. Recorder
# bursts can contain source observations 10-60 seconds old. Retain their exact
# clocks and let the sole worker obtain its independently fresh execution facts.
MAX_AGE_MS = 60000
TAIL_BYTES = 8 * 1024 * 1024


def clock(value):
    return int(datetime.fromisoformat(value.replace('Z', '+00:00')).timestamp() * 1000)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()


def event(raw):
    row = json.loads(raw)
    body = {k: v for k, v in row.items() if k != 'hash'}
    assert row['hash'] == sha256(canonical(body)).hexdigest(), 'ORIGINAL_RECORDER_ROW_HASH'
    e = row['event']
    assert e['source'] == 'official_sig_realtime', 'ORIGINAL_SIG_RECORDER_REQUIRED'
    return e


def normalize(raw, market, exchange, tournament, received, cutoff):
    assert str(raw['exchangeId']) == exchange
    source = clock(raw['asOf']['at'])
    received_ms = clock(received)
    assert 0 <= cutoff - received_ms <= MAX_AGE_MS, 'RESEARCH_BOOK_RECEIPT_AGE'
    assert 0 <= cutoff - source <= MAX_AGE_MS and source <= received_ms, 'RESEARCH_BOOK_SOURCE_AGE'
    sequence = raw['asOf']['sequence']
    assert type(sequence) is int and 0 <= sequence <= 9007199254740991
    levels = {}
    for side in ('bids', 'asks'):
        out, last = [], None
        assert type(raw[side]) is list and 0 < len(raw[side]) <= 100000
        for level in raw[side]:
            p, n = Decimal(str(level['price'])), level['quantity']
            assert p.is_finite() and 0 < p < 1 and p * 200 == (p * 200).to_integral_value()
            assert type(n) is int and 0 < n <= 9007199254740991
            assert last is None or (p < last if side == 'bids' else p > last)
            out.append(dict(price=format(p, '.6f'), quantity=n)); last = p
        levels[side] = out
    assert Decimal(levels['bids'][0]['price']) < Decimal(levels['asks'][0]['price'])
    return dict(tournamentId=tournament, marketId=market, exchangeId=exchange,
        sequence=str(sequence), sourceAt=raw['asOf']['at'], requestStartedAt=received,
        receivedAt=received, transport='official_sig_realtime', **levels)


def row_books(raw, mapping, tournament, cutoff):
    e = event(raw)
    if e['event'] != 'markets_batch':
        return {}
    out, seen = {}, set()
    for market in e['payload']['markets']:
        mid, b = str(market['marketId']), market['batch']
        assert mid not in seen, 'ORIGINAL_DUPLICATE_MARKET'; seen.add(mid)
        if b.get('resyncRequired') or b.get('marketSettled'):
            continue
        delivery = b['delivery']
        assert delivery['model'] == 'best-effort-authoritative-resync'
        assert type(delivery['revision']) is int and type(delivery['previousRevision']) is int
        assert delivery['revision'] > delivery['previousRevision'] >= 0
        for book in b.get('books', []):
            ex = str(book['exchangeId'])
            if mapping.get(ex) != mid:
                continue
            assert ex not in out, 'ORIGINAL_DUPLICATE_BOOK'
            dirty = [d for d in b.get('bookDirty', []) if str(d['exchangeId']) == ex]
            assert all(str(d['marketId']) == mid and d['tournamentId'] == tournament for d in dirty)
            if any(clock(d['at']) > clock(book['asOf']['at']) for d in dirty):
                continue
            try:
                out[ex] = normalize(book, mid, ex, tournament, e['receivedAt'], cutoff)
            except (AssertionError, ValueError, KeyError, ArithmeticError):
                # Unavailable observations are not valid executable prices.
                continue
    return out


def capture(runtime, pid, compiled, view, registry, cutoff, output):
    mapping = {r['exchangeId']: r['marketId'] for r in registry}
    assert all(view['scope']['exchanges'].get(ex) == mid for ex, mid in mapping.items())
    tournament = view['scope']['tournamentId']
    selected, originals = {}, {}
    paths = sorted(Path(runtime).glob(f'breadth-{pid}-*.jsonl'), key=lambda p: p.stat().st_mtime)[-2:]
    for path in paths:
        with path.open('rb') as stream:
            size = path.stat().st_size
            offset = max(0, size - TAIL_BYTES); stream.seek(offset)
            if offset:
                stream.readline()
            tail = stream.read(size - stream.tell())
        for raw in tail.splitlines(keepends=True):
            if not raw.endswith(b'\n'):
                continue
            e = event(raw)
            if clock(e['receivedAt']) > cutoff:
                continue
            if e['event'] == 'stream_reset':
                selected.clear()
            elif e['event'] in ('book_dirty', 'market_settled'):
                mid = str(e['payload']['marketId'])
                selected = {ex: item for ex, item in selected.items() if item['book']['marketId'] != mid}
            elif e['event'] == 'markets_batch':
                for market in e['payload']['markets']:
                    b = market['batch']
                    if b.get('resyncRequired') or b.get('marketSettled') or b.get('bookDirty'):
                        mid = str(market['marketId'])
                        selected = {ex: item for ex, item in selected.items() if item['book']['marketId'] != mid}
                candidates = row_books(raw, mapping, tournament, cutoff)
                for market in e['payload']['markets']:
                    for book in market['batch'].get('books', []):
                        ex = str(book['exchangeId'])
                        if ex not in candidates:
                            selected.pop(ex, None)
                if not candidates:
                    continue
                h = sha256(raw).hexdigest(); originals[h] = base64.b64encode(raw).decode()
                for ex, book in candidates.items():
                    previous = selected.get(ex)
                    if previous and (int(book['sequence']), clock(book['sourceAt'])) < (int(previous['book']['sequence']), clock(previous['book']['sourceAt'])):
                        continue
                    if previous and book['sequence'] == previous['book']['sequence'] and book['sourceAt'] == previous['book']['sourceAt']:
                        assert all(book[k] == previous['book'][k] for k in ('bids', 'asks')), 'ORIGINAL_BOOK_CONFLICT'
                    selected[ex] = dict(book=book, originalRowSha256=h)
    used = {r['originalRowSha256'] for r in selected.values()}
    result = dict(schema='sig_original_book_research_observations_v1', workerPid=pid,
        capturedAtMs=cutoff, maximumObservationAgeMs=MAX_AGE_MS, tournamentId=tournament,
        nativeReportSha256=compiled['reportSha256'], nativeViewSha256=compiled['viewSha256'],
        observations=selected, originalRows={h: originals[h] for h in sorted(used)},
        completeTradeHistoryClaimed=False, nativeAdmissionClaimed=False,
        atomicAccountAndBookSnapshotClaimed=False, executionAuthority=False, accountRequests=0)
    Path(output).write_bytes(canonical(result) + b'\n')
    return result


def load(inputs, compiled, registry):
    books = {b['exchangeId']: b for b in compiled['originalBooks']}
    path = Path(inputs) / 'research-book-observations.original.json'
    if not path.exists():
        return books, {}
    x = json.loads(path.read_bytes())
    assert x['schema'] == 'sig_original_book_research_observations_v1'
    assert x['nativeReportSha256'] == compiled['reportSha256'] and x['nativeViewSha256'] == compiled['viewSha256']
    assert x['maximumObservationAgeMs'] == MAX_AGE_MS
    assert all(x[k] is False for k in ('completeTradeHistoryClaimed', 'nativeAdmissionClaimed', 'atomicAccountAndBookSnapshotClaimed', 'executionAuthority'))
    # An old native cache entry must not keep winning the research screen and
    # consuming physical refresh packets after current raw evidence disappears.
    # Preserve the original compiled portfolio; select only observed fresh books.
    books = {ex: b for ex, b in books.items() if not b.get('invalid', False)
        and all(0 <= x['capturedAtMs'] - clock(b[k]) <= MAX_AGE_MS
                for k in ('sourceAt', 'receivedAt'))}
    mapping = {r['exchangeId']: r['marketId'] for r in registry}
    sources, parsed = {}, {}
    for ex, item in x['observations'].items():
        h = item['originalRowSha256']
        if h not in parsed:
            raw = base64.b64decode(x['originalRows'][h], validate=True)
            assert sha256(raw).hexdigest() == h, 'ORIGINAL_RESEARCH_ROW_CHANGED'
            parsed[h] = raw, row_books(raw, mapping, x['tournamentId'], x['capturedAtMs'])
        raw, candidates = parsed[h]
        assert candidates.get(ex) == item['book'], 'ORIGINAL_RESEARCH_BOOK_CHANGED'
        book = item['book']; previous = books.get(ex)
        if previous and (int(previous['sequence']), clock(previous['sourceAt'])) > (int(book['sequence']), clock(book['sourceAt'])):
            continue
        books[ex] = book
        sources[ex] = raw
    return books, sources
