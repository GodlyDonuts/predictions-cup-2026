"""Summarize the exact sealed root-run metadata probe; no network or live input."""
import argparse
from collections import Counter, defaultdict
import gzip
import hashlib
import json
from pathlib import Path

COMPRESSED = '74fcffafc3d6c56eb8be1143953c7365849a423e7bde9bd1056455d79c467ec6'
DECODED = '1cac8313f404504f49a8792c12fa1471ada9ab8a04b0b467ac5739c47c630a63'
MODULE = '4e4649c054a469baed4ba57651fae6c201aed8a479ba6f8551e5d28e0e0d891e'


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def quantiles(rows, key):
    values = sorted(r[key] for r in rows)
    return {str(q): values[int((len(values)-1)*q)] for q in (0, .1, .25, .5, .75, .9, 1)}


def summarize(path):
    before = path.stat(); raw = path.read_bytes(); decoded = gzip.decompress(raw)
    if sha(raw) != COMPRESSED or sha(decoded) != DECODED:
        raise ValueError('ORIGINAL_PROBE_REQUIRED')
    rows = [json.loads(line) for line in decoded.splitlines()]
    start, end = rows[0], rows[-1]
    if start['moduleSha256'] != MODULE or start['historyPrefixVerified'] is not False or end['accountRequests'] != 0:
        raise ValueError('PROBE_PROVENANCE')
    books = [r for r in rows if r['kind'] == 'bookObservation']
    trades = [r for r in rows if r['kind'] == 'publicTradeProbe']
    frames = defaultdict(list)
    for book in books:
        if book['sourceAgeMs'] != book['receiptAgeMs']+book['sourceToReceiptMs']:
            raise ValueError('CLOCK_EQUATION')
        frames[book['journalHash']].append(book)
    details = [{'sequence': group[0]['journalSequence'], 'hash': h, 'bookCount': len(group), 'originalReceivedAt': group[0]['receivedAt'], 'firstProbeObservationMs': min(r['observationMs'] for r in group), 'lastProbeObservationMs': max(r['observationMs'] for r in group), 'receiptAgeMsMin': min(r['receiptAgeMs'] for r in group), 'receiptAgeMsMax': max(r['receiptAgeMs'] for r in group)} for h, group in frames.items()]
    report = {'schemaVersion': 1, 'source': {'compressedSha256': COMPRESSED, 'decodedSha256': DECODED, 'compressedBytes': len(raw), 'decodedBytes': len(decoded), 'observerModuleSha256': MODULE},
              'probeDurationSeconds': end['elapsedSeconds'], 'rows': len(rows), 'bookObservations': len(books), 'distinctFrames': len(frames), 'distinctExchanges': len({r['exchangeId'] for r in books}),
              'acceptedBookObservations': sum(r['observerAccepted'] for r in books), 'sameSequenceNewerSourceObserved': sum(r['sameSequenceNewerSource'] for r in books),
              'sourceAgeMsQuantiles': quantiles(books, 'sourceAgeMs'), 'receiptAgeMsQuantiles': quantiles(books, 'receiptAgeMs'), 'sourceToReceiptMsQuantiles': quantiles(books, 'sourceToReceiptMs'),
              'sourceAgeOver3000ms': sum(r['sourceAgeMs'] > 3000 for r in books), 'receiptAgeOver3000ms': sum(r['receiptAgeMs'] > 3000 for r in books), 'sourceOldReceiptWithin3000ms': sum(r['sourceAgeMs'] > 3000 and r['receiptAgeMs'] <= 3000 for r in books),
              'frames': details, 'publicTradeProbes': len(trades), 'tradeProbeClassifications': dict(Counter(r['classification'] for r in trades)),
              'matchedTradeBookAges': [{'tradeSourceAgeMs': r['probeFirstAvailableMs']-r['trade']['sourceTsMs'], 'bookSourceAgeMs': b['sourceAgeMs'], 'bookReceiptAgeMs': b['receiptAgeMs']} for r in trades for b in r['matchedSig']],
              'originalEndCounters': end['counts'], 'originalObserverCounters': end['observerCounters'], 'accountRequests': 0, 'historyPrefixVerified': False, 'pricingAuthority': False, 'observerNormalizerChanged': False, 'sigWorkerChanged': True, 'sigWorkerBeforeSource': '2225b1efba5a16078cea875f5162f96742514d3a', 'sigWorkerAfterSource': 'b112c6c1b941bcc3282c754c1883a55b2b9b6cc2',
              'reproducerLfSha256': sha(Path(__file__).read_bytes().replace(b'\r\n', b'\n')),
              'limitations': ['Fresh suffix only; absent target book is not account-wide or full-history absence.', 'Book-weighted lag summaries share frames and are not independent statistical samples.', 'Receipt-to-probe lag combines producer queue/append, filesystem/tail scheduling, cache HTTP and probe work; no append clock isolates components.', 'No continuous no-change witness, repaired clock, measured future model gain or actual fill inference.']}
    after = path.stat()
    if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns) or sha(path.read_bytes()) != COMPRESSED:
        raise ValueError('SOURCE_CHANGED')
    return report


if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--input', required=True, type=Path); p.add_argument('--output', required=True, type=Path)
    a = p.parse_args(); a.output.write_text(json.dumps(summarize(a.input), indent=2, sort_keys=True)+'\n', encoding='utf-8')
