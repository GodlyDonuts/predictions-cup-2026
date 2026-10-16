"""Prospective cross-venue observations; reads existing files/cache, never SIG HTTP.

This process has no credentials, execution API, fills, or profit authority.
Training features are frozen at local decision time; future labels use later
original SIG source/receipt clocks. Old records never become fresh on restart.
"""
import argparse
import collections
import datetime as dt
from decimal import Decimal, InvalidOperation
import gzip
import hashlib
import json
import math
import os
from pathlib import Path
import re
import stat
import time
import urllib.request


def compact(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)


def stamp(ms):
    return dt.datetime.fromtimestamp(ms / 1000, dt.timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def millis(value):
    parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        raise ValueError("OBSERVER_CLOCK")
    return int(parsed.timestamp() * 1000)


def decimal(value):
    if isinstance(value, bool) or not isinstance(value, (str, int)):
        raise ValueError("OBSERVER_DECIMAL")
    result = Decimal(value)
    if not result.is_finite() or result < 0 or result.as_tuple().exponent < -6:
        raise ValueError("OBSERVER_DECIMAL")
    return result


def levels(rows, ascending):
    result = []
    for row in rows:
        p, q = decimal(row["price"]), decimal(row["quantity"])
        if p > 1 or q <= 0 or (result and (p <= result[-1][0] if ascending else p >= result[-1][0])):
            raise ValueError("OBSERVER_BOOK")
        result.append((p, q))
    return result


def features(bids, asks):
    if not bids or not asks or bids[0][0] > asks[0][0]:
        raise ValueError("OBSERVER_BOOK")
    bp, bq = bids[0]
    ap, aq = asks[0]
    midpoint = float((bp + ap) / 2)
    return {"bid": str(bp), "ask": str(ap), "bidQuantity": str(bq), "askQuantity": str(aq),
            "midpoint": midpoint, "spread": float(ap - bp),
            "imbalance": float((bq - aq) / (bq + aq)),
            "microprice": float((bp * aq + ap * bq) / (bq + aq))}


def logit(p):
    p = min(.999999, max(.000001, p))
    return math.log(p / (1 - p))


class Tail:
    """Fresh suffix only. A checked suffix chain is not a full-history audit."""
    def __init__(self, path):
        flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
        self.fd = os.open(path, flags)
        info = os.fstat(self.fd)
        if not stat.S_ISREG(info.st_mode):
            raise ValueError("OBSERVER_NOT_REGULAR")
        self.identity = (info.st_dev, info.st_ino)
        self.path, self.position, self.buffer = Path(path), info.st_size, b""
        self.skip = bool(info.st_size and os.pread(self.fd, 1, info.st_size - 1) != b"\n") if hasattr(os, "pread") else False
        if info.st_size and not hasattr(os, "pread"):
            os.lseek(self.fd, info.st_size - 1, os.SEEK_SET)
            self.skip = os.read(self.fd, 1) != b"\n"
        os.lseek(self.fd, info.st_size, os.SEEK_SET)
        self.previous, self.sequence = None, None

    def read(self):
        info = self.path.lstat()
        if not stat.S_ISREG(info.st_mode) or (info.st_dev, info.st_ino) != self.identity or info.st_size < self.position:
            raise ValueError("OBSERVER_JOURNAL_CHANGED")
        chunk = os.read(self.fd, 4 * 1024 * 1024)
        self.position += len(chunk)
        self.buffer += chunk
        if len(self.buffer) > 12 * 1024 * 1024:
            raise ValueError("OBSERVER_LINE_LIMIT")
        lines = self.buffer.split(b"\n")
        self.buffer = lines.pop()
        for line in lines:
            if self.skip:
                self.skip = False
                continue
            if len(line) > 8 * 1024 * 1024:
                raise ValueError("OBSERVER_LINE_LIMIT")
            row = json.loads(line)
            unsigned = {k: v for k, v in row.items() if k != "hash"}
            if hashlib.sha256(compact(unsigned).encode()).hexdigest() != row.get("hash"):
                raise ValueError("OBSERVER_JOURNAL_HASH")
            if self.previous is not None and (row["previous"] != self.previous or row["sequence"] != self.sequence + 1):
                raise ValueError("OBSERVER_JOURNAL_GAP")
            self.previous, self.sequence = row["hash"], row["sequence"]
            yield row

    def close(self):
        os.close(self.fd)


class Study:
    def __init__(self, emit, horizons=(1000, 3000, 10000, 30000, 120000), sig_age=3000, external_age=20000, label_slack=3000):
        self.emit, self.horizons = emit, horizons
        self.sig_age, self.external_age, self.label_slack = sig_age, external_age, label_slack
        self.external, self.history = {}, collections.defaultdict(collections.deque)
        self.pending = collections.defaultdict(collections.deque)
        self.last_sample, self.last_source = {}, {}
        self.counts = collections.Counter()
        self.markets = set()

    def external_update(self, envelope, now):
        # Fetch completion is when this observer first has these features.
        for row in envelope["markets"]:
            received = millis(row["receivedAt"])
            if received > now or now - received > self.external_age:
                self.counts["externalStaleOrFuture"] += 1
                continue
            if not isinstance(row.get("bodySha256"), str) or len(row["bodySha256"]) != 64:
                raise ValueError("OBSERVER_EXTERNAL_PROVENANCE")
            n = row["normalized"]
            try:
                f = features(levels(n["yesBids"], False), levels(n["yesAsks"], True))
            except (ValueError, InvalidOperation, KeyError):
                self.counts["externalInvalidOrOneSided"] += 1
                continue
            value = {"ticker": row["ticker"], "mappingKind": row["mappingKind"], "receivedMs": received,
                     "availableMs": now, "bodySha256": row["bodySha256"], "features": f,
                     "pricingAuthority": False}
            for market in row["sigMarketIds"]:
                market = str(market)
                prior = self.external.get(market)
                if prior and received < prior["receivedMs"]:
                    self.counts["externalClockRegression"] += 1
                    continue
                if prior and received == prior["receivedMs"]:
                    if prior["bodySha256"] != value["bodySha256"]:
                        raise ValueError("OBSERVER_EXTERNAL_CONFLICT")
                    continue  # Retain first local availability; no renewed clock.
                self.external[market] = value
                h = self.history[market]
                h.append(value)
                while h and h[0]["receivedMs"] < now - 180000:
                    h.popleft()
        self.counts["externalPolls"] += 1

    def accept(self, row, now):
        event = row["event"]
        if event.get("type") == "gap" or (event.get("type") == "coverage" and event.get("complete") is not True):
            # Incomplete scope coverage invalidates that scope's horizon path.
            # A complete trade-tape coverage receipt is not a global stream reset.
            self.counts["coverageBoundaries"] += 1
            keys = [key for key in self.pending if
                    (not event.get("marketId") or key[0] == str(event["marketId"])) and
                    (not event.get("exchangeId") or key[1] == str(event["exchangeId"]))]
            for key in keys:
                self.counts["labelsMissingTimelyBook"] += sum(len(s["remaining"]) for s in self.pending.pop(key))
                self.last_source.pop(key, None)
            return
        if event.get("type") != "book":
            return
        b = event["book"]
        market, exchange = str(b["marketId"]), str(b["exchangeId"])
        source, received = millis(b["sourceAt"]), millis(b["receivedAt"])
        key = (market, exchange)
        if source > received or received > now:
            self.counts["sigFuture"] += 1
            return
        revision = int(b["sequence"])
        if revision <= self.last_source.get(key, -1):
            self.counts["sigDuplicateOrReordered"] += 1
            return
        self.last_source[key] = revision
        try:
            bids, asks = levels(b["bids"], False), levels(b["asks"], True)
            f = features(bids, asks)
        except (ValueError, InvalidOperation, KeyError):
            self.counts["sigInvalidOrOneSided"] += 1
            return
        self.counts["sigBooks"] += 1
        waiting = self.pending[key]
        for sample in list(waiting):
            for horizon in list(sample["remaining"]):
                target = sample["decisionMs"] + horizon
                if source < target or received < target:
                    continue
                sample["remaining"].remove(horizon)
                if source - target > self.label_slack or received - target > self.label_slack:
                    self.counts["labelsMissingTimelyBook"] += 1
                    continue
                before = sample["sig"]["features"]
                label = {"kind": "horizon_label", "sampleId": sample["sampleId"], "horizonMs": horizon,
                         "sourceAt": b["sourceAt"], "receivedAt": b["receivedAt"], "observedAt": stamp(now),
                         "sigJournalHash": row["hash"], "features": f,
                         "logitMidpointChange": logit(f["midpoint"]) - logit(before["midpoint"]),
                         "yesOneShareTakerRoundTrip": str(bids[0][0] - Decimal(before["ask"])) if bids[0][1] >= 1 and Decimal(before["askQuantity"]) >= 1 else None,
                         "noOneShareTakerRoundTrip": str(Decimal(before["bid"]) - asks[0][0]) if asks[0][1] >= 1 and Decimal(before["bidQuantity"]) >= 1 else None,
                         "counterfactual": True, "feesAssumed": "0", "actualFill": False}
                self.emit(label)
                self.counts["labels"] += 1
            if not sample["remaining"] or now > sample["decisionMs"] + max(self.horizons) + self.label_slack:
                waiting.remove(sample)
        # Samples are per-market at most once/second, not every journal update.
        if now - received > self.sig_age or now - source > self.sig_age or now - self.last_sample.get(key, -10000) < 1000:
            return
        ext = self.external.get(market)
        if not ext or now - ext["receivedMs"] > self.external_age or ext["availableMs"] > now:
            self.counts["noCurrentExternal"] += 1
            return
        sample_id = hashlib.sha256(f"{row['hash']}:{ext['bodySha256']}:{now}".encode()).hexdigest()
        changes = {}
        for ms in (1000, 5000, 30000):
            prior = next((v for v in reversed(self.history[market]) if v["receivedMs"] <= now - ms), None)
            changes[str(ms)] = (logit(ext["features"]["midpoint"]) - logit(prior["features"]["midpoint"])) if prior and now - ms - prior["receivedMs"] <= self.external_age else None
        sample = {"kind": "decision_features", "sampleId": sample_id, "decisionMs": now, "decisionAt": stamp(now),
                  "marketId": market, "exchangeId": exchange, "sig": {"journalHash": row["hash"], "journalSequence": row["sequence"],
                  "sourceAt": b["sourceAt"], "receivedAt": b["receivedAt"], "features": f},
                  "external": ext, "externalLogitChanges": changes, "pricingAuthority": False,
                  "executionAuthority": False, "horizonsMs": list(self.horizons)}
        self.emit(sample)
        waiting.append({**sample, "remaining": set(self.horizons)})
        self.last_sample[key] = now
        self.markets.add(market)
        self.counts["samples"] += 1
        # Bound unmatched labels during quiet/stopped markets. No synthetic mark.
        for scope in list(self.pending):
            q = self.pending[scope]
            while q and now > q[0]["decisionMs"] + max(self.horizons) + self.label_slack:
                old = q.popleft()
                self.counts["labelsMissingTimelyBook"] += len(old["remaining"])


class Output:
    def __init__(self, root):
        self.root, self.stream, self.segment, self.bytes = root, None, None, 0

    def emit(self, row):
        segment = time.strftime("%Y%m%dT%H", time.gmtime())
        if segment != self.segment or self.bytes >= 64 * 1024 * 1024:
            self.close()
            self.prune()
            name = f"{segment}-{time.time_ns()}.jsonl.gz"
            self.stream = gzip.open(self.root / name, "xt", encoding="utf-8", compresslevel=1)
            self.segment, self.bytes = segment, 0
        line = compact(row) + "\n"
        self.stream.write(line)
        self.bytes += len(line)

    def prune(self):
        files = []
        for p in self.root.iterdir():
            if not re.fullmatch(r"\d{8}T\d{2}-\d+\.jsonl\.gz", p.name):
                continue
            info = p.lstat()
            if stat.S_ISREG(info.st_mode):
                files.append((info.st_mtime, info.st_size, p))
        total = sum(v[1] for v in files)
        for modified, size, p in sorted(files):
            if total <= 2 * 1024 ** 3 and time.time() - modified <= 72 * 3600:
                break
            p.unlink()
            total -= size

    def close(self):
        if self.stream:
            self.stream.close()
            self.stream = None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--journal", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--duration-seconds", type=int, default=0)
    args = parser.parse_args()
    # Unix VPS read-only sidecar, advisory lock kept on the same inode.
    import fcntl
    args.output.mkdir(parents=True, exist_ok=True, mode=0o700)
    lock = (args.output / "observer.lock").open("a+")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    tail, output = Tail(args.journal), Output(args.output)
    study = Study(output.emit)
    started, last_poll, last_status = time.monotonic(), 0, 0
    try:
        while not (args.output / "STOP").exists():
            if args.duration_seconds and time.monotonic() - started >= args.duration_seconds:
                break
            if time.monotonic() - last_poll >= 1:
                last_poll = time.monotonic()
                try:
                    request = urllib.request.Request("http://127.0.0.1:17841/v1/quotes")
                    with urllib.request.urlopen(request, timeout=2) as response:
                        raw = response.read(16 * 1024 * 1024 + 1)
                    if len(raw) > 16 * 1024 * 1024:
                        raise ValueError("OBSERVER_CACHE_LIMIT")
                    study.external_update(json.loads(raw), int(time.time() * 1000))
                except (OSError, TimeoutError):
                    study.counts["externalUnavailable"] += 1
            for row in tail.read():
                study.accept(row, int(time.time() * 1000))
            if time.monotonic() - last_status >= 5:
                last_status = time.monotonic()
                status = {"at": stamp(int(time.time() * 1000)), "state": "observing", "pid": os.getpid(),
                          "counters": dict(study.counts), "observedMarkets": len(study.markets),
                          "pendingSamples": sum(map(len, study.pending.values())), "journalPosition": tail.position,
                          "journalSequence": tail.sequence, "originalJournalHash": tail.previous,
                          "readOnly": True, "executionAuthority": False, "historyPrefixVerified": False}
                temp = args.output / "status.json.tmp"
                temp.write_text(compact(status) + "\n", encoding="utf-8")
                os.replace(temp, args.output / "status.json")
            time.sleep(.1)
    finally:
        output.close()
        tail.close()
        lock.close()


if __name__ == "__main__":
    main()
