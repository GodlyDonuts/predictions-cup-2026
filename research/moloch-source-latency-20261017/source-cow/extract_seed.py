"""Finite private benchmark extraction from a previously authenticated native prefix.

No account access, financial replay, source admission or runtime mutation. The
caller supplies the independently authenticated exact prefix bytes and SHA256.
JSON trade value tokens are retained; floats are never rewritten into fixtures.
"""
import argparse
from decimal import Decimal
import hashlib
import json
from pathlib import Path


def need(value, code):
    if not value:
        raise ValueError(code)


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--journal', required=True, type=Path)
    p.add_argument('--prefix-bytes', required=True, type=int)
    p.add_argument('--prefix-sha256', required=True)
    p.add_argument('--control', required=True, type=Path)
    p.add_argument('--control-sha256', required=True)
    p.add_argument('--output-directory', required=True, type=Path)
    a = p.parse_args()
    need(a.prefix_bytes > 0 and len(a.prefix_sha256) == 64, 'PREFIX_PIN')
    control_raw = a.control.read_bytes()
    need(hashlib.sha256(control_raw).hexdigest() == a.control_sha256, 'CONTROL_PIN')
    control = json.loads(control_raw)
    decoder = json.JSONDecoder(parse_float=Decimal)
    retained = {}
    digest = hashlib.sha256()
    count = 0
    read = 0
    # Session::remember preserves the FIRST event for each JSON-display key.
    with a.journal.open('rb') as f:
        while read < a.prefix_bytes:
            raw = f.readline(a.prefix_bytes - read + 1)
            need(raw and raw.endswith(b'\n') and read + len(raw) <= a.prefix_bytes,
                 'INCOMPLETE_OR_OVERSIZED_PREFIX')
            read += len(raw)
            digest.update(raw)
            count += 1
            if b'"type":"trade"' not in raw:
                continue
            s = raw.decode('utf-8')
            at = s.index('"event":') + len('"event":')
            event, end = decoder.raw_decode(s, at)
            if event.get('type') != 'trade':
                continue
            need(isinstance(event['exchangeId'], str) and isinstance(event['id'], str),
                 'STRING_TRADE_ID_REQUIRED')
            display = lambda v: json.dumps(v, ensure_ascii=False, separators=(',', ':'))
            key = display(event['exchangeId']) + ':' + display(event['id'])
            retained.setdefault(key, s[at:end].encode('utf-8'))
    need(read == a.prefix_bytes and digest.hexdigest() == a.prefix_sha256, 'PREFIX_HASH')
    need(retained, 'NO_SOURCE_FACTS')
    pairs = [dict(exchangeId=e, marketId=m)
             for e, m in sorted(control['scope']['exchanges'].items())]
    a.output_directory.mkdir(parents=True, exist_ok=False)
    seed = b'[' + b','.join(retained[k] for k in sorted(retained)) + b']\n'
    scopes = (json.dumps({'tournamentId': control['scope']['tournamentId'],
                          'scopes': pairs}, separators=(',', ':')) + '\n').encode()
    files = {'seed-events.original.json': seed, 'scopes.original.json': scopes}
    proof = dict(prefixBytes=read, prefixSha256=digest.hexdigest(), nativeRows=count,
                 uniqueRetainedTrades=len(retained), scopes=len(pairs),
                 admission='previously independently authenticated prefix SHA; no new financial replay',
                 firstEventPerSessionDisplayKey=True, originalEventTokensPreserved=True,
                 currentLiveRestSourceHeapReconstructed=False, executionAuthority=False,
                 files={n: dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())
                        for n, b in files.items()})
    for name, raw in files.items():
        with (a.output_directory / name).open('xb') as f:
            f.write(raw)
    with (a.output_directory / 'extraction-proof.json').open('xb') as f:
        f.write((json.dumps(proof, indent=2) + '\n').encode())
    print(json.dumps(proof))


if __name__ == '__main__':
    main()
