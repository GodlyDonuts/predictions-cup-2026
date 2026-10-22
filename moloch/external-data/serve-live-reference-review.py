"""Localhost-only public observation relay for an authenticated SSH tunnel."""
import argparse
import gzip
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
import os
from pathlib import Path
import time
from urllib.parse import parse_qs, urlparse


def rank_cache(config):
    """Read public quote originals only; issue time does not renew source clocks."""
    markets, total, unavailable = [], 0, []
    for path in sorted((Path(config['kalshiRoot']) / 'latest').glob('*.json')):
        if path.stat().st_size > 4 * 1024 * 1024:
            raise ValueError('RANK_CACHE_MEMBER_BOUND')
        raw = path.read_bytes()
        total += len(raw)
        if total > 32 * 1024 * 1024:
            raise ValueError('RANK_CACHE_TOTAL_BOUND')
        row = json.loads(raw)
        if row.get('status') != 200:
            unavailable.append(path.stem)
            continue
        if (path.stem != row['ticker']
                or hashlib.sha256(row['bodyText'].encode()).hexdigest() != row['bodySha256']
                or json.loads(row['bodyText']) != row['body']):
            raise ValueError('RANK_CACHE_ORIGINAL_BODY')
        markets.append(row)
    if not markets:
        raise ValueError('RANK_CACHE_EMPTY')
    from datetime import datetime, timezone
    return dict(at=datetime.now(timezone.utc).isoformat(), markets=markets,
        unavailableTickers=unavailable, officialSigRequests=0, newVenueRequests=0,
        sourceClocksRenewed=False, executionAuthority=False)

spec = importlib.util.spec_from_file_location('reference_capture', Path(__file__).with_name('prepare-live-reference-review.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', type=Path, required=True)
    a = parser.parse_args()
    c = json.loads(a.config.read_bytes())
    allowed = set(c['exchanges'])
    status = {'pid': os.getpid(), 'startedAtMs': time.time_ns() // 1_000_000,
        'captures': 0, 'errors': 0, 'officialSigRequests': 0, 'mlFitOrSelectionUse': False}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            url = urlparse(self.path)
            code, zipped = 200, False
            try:
                if url.path == '/status':
                    value = dict(status)
                elif url.path == '/rank-cache' and not url.query:
                    value = rank_cache(c)
                    zipped = True
                elif url.path == '/capture':
                    query = parse_qs(url.query, strict_parsing=True)
                    if set(query) != {'exchanges'} or len(query['exchanges']) != 1:
                        raise ValueError('EXACT_CAPTURE_QUERY')
                    selected = query['exchanges'][0].split(',')
                    if not selected or len(selected) != len(set(selected)) or not set(selected) <= allowed:
                        raise ValueError('CAPTURE_SCOPE')
                    value = module.capture(Path(c['registryRoot']), Path(c['kalshiRoot']),
                        Path(c['polymarketUniverse']), Path(c['polymarketCache']), selected, time.time_ns() // 1_000_000)
                    status.update(captures=status['captures'] + 1, lastCaptureAtMs=value['capturedAtMs'])
                    zipped = True
                else:
                    code, value = 404, {'error': 'NOT_FOUND'}
            except Exception as e:
                status['errors'] += 1
                code, value = 503, {'error': type(e).__name__, 'reason': str(e)[:500]}
            raw = json.dumps(value, separators=(',', ':')).encode()
            if zipped:
                raw = gzip.compress(raw)
            self.send_response(code)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Cache-Control', 'no-store')
            if zipped:
                self.send_header('Content-Encoding', 'gzip')
            self.send_header('Content-Length', str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

    server = ThreadingHTTPServer(('127.0.0.1', c['port']), Handler)
    print(json.dumps(status), flush=True)
    server.serve_forever()


if __name__ == '__main__':
    main()
