"""Copy public reference cache originals for finite rank research; no API calls.

Capture time never replaces individual request/receipt/publication times. The
result is an evidence bundle, not an approval, forecast or live order input.
"""
import argparse
import base64
from datetime import datetime, timezone
import gzip
from hashlib import sha256
import json
import os
from pathlib import Path


def run(args):
    os.umask(0o077)
    args.output.mkdir(mode=0o700, parents=True, exist_ok=False)
    rows, rejected = [], []
    originals = args.output / "quote-originals"
    originals.mkdir(mode=0o700)
    for path in sorted((args.cache / "latest").glob("*.json")):
        raw = path.read_bytes()
        row = json.loads(raw)
        if row.get("status") != 200:
            rejected.append(path.name)
            continue
        assert path.stem == row["ticker"], "CACHE_TICKER_PATH"
        assert sha256(row["bodyText"].encode()).hexdigest() == row["bodySha256"], "CACHE_BODY_PIN"
        assert json.loads(row["bodyText"]) == row["body"], "CACHE_BODY_BINDING"
        start = datetime.fromisoformat(row["requestStartedAt"].replace("Z", "+00:00"))
        end = datetime.fromisoformat(row["receivedAt"].replace("Z", "+00:00"))
        assert start.utcoffset() is not None and end.utcoffset() is not None, "CACHE_TIMEZONE"
        assert start <= end <= datetime.now(timezone.utc), "CACHE_CLOCK"
        (originals / path.name).write_bytes(raw)
        rows.append(row)
    assert rows, "NO_VALID_QUOTE_ORIGINALS"
    at = datetime.now(timezone.utc).isoformat()
    cache = json.dumps({"at": at, "markets": rows}, separators=(",", ":")).encode()
    field_raw = args.field.read_bytes()
    field = json.loads(field_raw)
    assert field["executionAuthority"] is False and field["accountApiRequests"] == 0, "PUBLIC_FIELD_CLASSIFICATION"
    assert datetime.fromisoformat(field["receivedAt"].replace("Z", "+00:00")) <= datetime.fromisoformat(at), "FIELD_FUTURE"
    package = json.dumps({"cacheText": cache.decode(), "fieldText": field_raw.decode(),
        "cacheSha256": sha256(cache).hexdigest(), "newVenueRequests": 0}, separators=(",", ":")).encode()
    (args.output / "kalshi-cache-quotes.original.json").write_bytes(cache)
    (args.output / "field.original.json").write_bytes(field_raw)
    (args.output / "quotes.b64").write_text(base64.b64encode(gzip.compress(package)).decode())
    result = {"at": at, "markets": len(rows), "rejectedFiles": rejected,
        "cacheSha256": sha256(cache).hexdigest(), "packageSha256": sha256(package).hexdigest(),
        "sourceClocksRenewed": False, "newVenueRequests": 0, "executionAuthority": False}
    (args.output / "capture.public.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--cache", type=Path, required=True)
    p.add_argument("--field", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    run(p.parse_args())
