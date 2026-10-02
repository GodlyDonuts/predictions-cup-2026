"""Verify published bytes and structural facts offline; no network dependencies."""
import collections
import gzip
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent

def sha256(data):
    return hashlib.sha256(data).hexdigest()

def main():
    manifest = json.loads((ROOT / 'bundle_manifest.json').read_text())
    for name, expected in manifest['files'].items():
        path = ROOT / name
        assert path.resolve().is_relative_to(ROOT), name
        data = path.read_bytes()
        assert len(data) == expected['bytes'], (name, 'size mismatch')
        assert sha256(data) == expected['sha256'], (name, 'SHA-256 mismatch')
        assert hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest() == expected['git_blob_sha1'], (name, 'Git blob mismatch')
    packed = (ROOT / 'reference.json.gz').read_bytes()
    assert packed[:3] == b'\x1f\x8b\x08' and packed[3] == 0 and packed[4:8] == b'\0' * 4
    data = gzip.decompress(packed)
    validation = json.loads((ROOT / 'publication_validation.json').read_text())
    assert len(data) == validation['reference_json']['bytes']
    assert sha256(data) == validation['reference_json']['sha256']
    rows = json.loads(data)['markets']
    assert len(rows) == len({r['ticker'] for r in rows}) == 993
    assert len({r['race_key'] for r in rows}) == len({r['event_ticker'] for r in rows}) == 496
    assert collections.Counter(r['party'] for r in rows) == {'Democratic': 496, 'Republican': 496, 'Independent': 1}
    assert all(r['election_year'] == 2026 and r['status'] == 'active' and r['party_rule_verified'] for r in rows)
    assert all(r['source_quote_timestamp'] is None and r['source_quote_age_seconds'] is None and not r['metadata_updated_time_is_quote_time'] for r in rows)
    assert len(json.loads((ROOT / 'orderbook_snapshots.json').read_text())['books']) == 6
    assert len(json.loads((ROOT / 'candidate_or_nonstandard_markets.json').read_text())['markets']) == 14
    evidence = json.loads((ROOT / 'source_evidence_manifest.json').read_text())
    for entry in evidence['files']:
        published = entry['published_as']
        if published is None:
            continue
        contents = (ROOT / published).read_bytes()
        if entry['representation'] == 'gzip_uncompressed_bytes':
            contents = gzip.decompress(contents)
        assert len(contents) == entry['bytes'] and sha256(contents) == entry['sha256'], entry['source_file']
    print(f"PASS: {len(manifest['files'])} manifest-listed files; 993 unique contracts, 496 races, 6 books, 14 separate nonstandard contracts; gzip/hash/source-copy checks")
    print('Scope: derived bundle verification only; omitted raw catalog/event responses are not reproduced or independently authenticated')

if __name__ == '__main__':
    main()
