"""One-shot fresh monetary research after the existing worker recovers.

Uses the existing localhost public-data tunnel, never a SIG account client.
Produces an unapproved nomination only; it does not touch live owner files.
"""
import argparse
import gzip
from hashlib import sha256
import importlib.util
import json
import os
from pathlib import Path
import time
from types import SimpleNamespace
import urllib.request


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def run(a):
    os.umask(0o077)
    a.output.mkdir(mode=0o700, parents=True, exist_ok=False)
    deployment = json.loads(a.deployment.read_bytes())
    deadline = time.monotonic() + a.wait_seconds
    while time.monotonic() < deadline:
        assert Path('/proc', str(deployment['pid'])).exists(), 'EXPECTED_WORKER_EXITED'
        assert not (a.runtime / 'EXECUTION_STOP').exists(), 'EXECUTION_STOP_PRESENT'
        progress = json.loads((a.runtime / 'progress.json').read_bytes())
        if progress.get('pid') == deployment['pid'] and progress.get('phase') == 'trading' and progress.get('realtimeHealthy') is True:
            break
        time.sleep(5)
    else:
        raise RuntimeError('FINITE_RECOVERY_WAIT_EXPIRED')
    capture = module('current_money_capture', a.tools / 'capture-current-rank-inputs.py')
    inputs = a.output / 'inputs'
    capture.run(SimpleNamespace(source=a.source, runtime=a.runtime, reference=a.reference, output=inputs,
        pid=deployment['pid'], binary_sha256=deployment['binarySha256'], wait_seconds=180))
    # This is an existing local cache relay through the existing SSH tunnel.
    with urllib.request.urlopen('http://127.0.0.1:18964/rank-cache', timeout=15) as response:
        assert response.status == 200 and response.headers.get('Content-Encoding') == 'gzip'
        compressed = response.read(32 * 1024 * 1024 + 1)
        assert len(compressed) <= 32 * 1024 * 1024
    raw = gzip.decompress(compressed)
    assert len(raw) <= 32 * 1024 * 1024
    cache = json.loads(raw)
    assert cache['officialSigRequests'] == 0 and cache['newVenueRequests'] == 0 and cache['sourceClocksRenewed'] is False
    (inputs / 'kalshi-cache-quotes.original.json').write_bytes(raw)
    review_raw = (inputs / 'active-fourteen-reviews.private.json').read_bytes()
    (inputs / 'external-input-capture.json').write_text(json.dumps(dict(quoteSha256=sha256(raw).hexdigest(),
        reviewProjectionSha256=sha256(review_raw).hexdigest(), sourceClocksRenewed=False, newVenueRequests=0)) + '\n')
    builder = module('current_money_reference', a.source / 'research/tartarus-20261017/current-reference-model.py')
    reference = builder.build(inputs, a.source / 'originals', a.field, directional=True, historical_review_envelopes=True)
    (inputs / 'issued-reference-model.private.json').write_text(json.dumps(reference, indent=2) + '\n')
    scanner = module('current_money_scan', a.tools / 'scan-current-money-opportunities.py')
    candidates_path = inputs / 'direct-opportunities.public.json'
    scanned = scanner.run(SimpleNamespace(source=a.source, inputs=inputs, output=candidates_path,
        minimum_expected_money=a.minimum_expected_money))
    if not scanned['candidates']:
        result = dict(state='no_positive_declared_first_order', submitted=False, approvalPublished=False)
    else:
        selected = scanned['candidates'][0]['leg']
        preparer = module('current_money_prepare', a.tools / 'prepare-current-money-nomination.py')
        target = a.output / 'prepared'
        preparer.run(SimpleNamespace(source=a.source, inputs=inputs, candidates=candidates_path, output=target,
            exchange=selected['ex'], side=selected['side'], action=selected['action'], decision_ms=time.time_ns() // 1000000,
            horizon_ms=300000, minimum_expected_money=a.minimum_expected_money, price_tolerance=a.price_tolerance))
        result = dict(state='fresh_unapproved_nomination_prepared',
            nomination=json.loads((target / 'prepared.public.json').read_bytes()),
            originalKnownOrders=json.loads((inputs / 'compiled-current.private.json').read_bytes())['knownUnclosedAcknowledgments'],
            submitted=False, approvalPublished=False)
    result.update(expectedWorkerPid=deployment['pid'], expectedBinarySha256=deployment['binarySha256'],
        newAccountClients=0, executionAuthority=False)
    (a.output / 'result.public.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result), flush=True)


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('source', 'tools', 'runtime', 'reference', 'deployment', 'field', 'output'):
        p.add_argument('--' + name, type=Path, required=True)
    p.add_argument('--wait-seconds', type=int, default=2700)
    p.add_argument('--minimum-expected-money', default='0.010000')
    p.add_argument('--price-tolerance', default='0.010000')
    run(p.parse_args())
