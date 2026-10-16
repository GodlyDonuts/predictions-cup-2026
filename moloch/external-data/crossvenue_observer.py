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
import signal
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


FLOW_FIELDS = ('tradeId', 'ticker', 'bodySha256', 'receivedMs', 'availableMs', 'sourceTsMs',
               'sid', 'sequence', 'streamEpoch', 'quantity', 'yesPrice', 'noPrice', 'direction', 'isBlockTrade')


def validate_trade_flow(flow, ticker, now, epoch=None):
    """Validate optional original exported evidence, never a complete trade tape."""
    def need(ok):
        if not ok:
            raise ValueError('OBSERVER_TRADE_FLOW')
    def integer(value):
        return type(value) is int and 0 <= value <= 9007199254740991
    def signed(value):
        need(isinstance(value, str) and bool(re.fullmatch(r'-?\d+(?:\.\d{1,6})?', value)))
        return Decimal(value)
    need(isinstance(ticker, str) and bool(ticker) and isinstance(flow, dict) and flow.get('source') == 'kalshi_official_ws_v2_public_trade' and flow.get('ticker') == ticker)
    at = flow.get('asOfMs')
    need(integer(at) and at <= now and now-at <= 2000 and flow.get('observing') is True)
    need(integer(flow.get('streamEpoch')) and flow['streamEpoch'] >= 1 and (epoch is None or epoch == flow['streamEpoch']))
    need(flow.get('sid') is None or integer(flow['sid']) and flow['sid'] >= 1)
    need(flow.get('pricingAuthority') is False and flow.get('executionAuthority') is False and flow.get('bookFreshnessRenewed') is False)
    need(isinstance(flow.get('windows'), dict) and set(flow['windows']) == {'1000', '5000', '30000'})
    retained_evidence = {}
    for text_ms, w in flow['windows'].items():
        ms = int(text_ms)
        need(isinstance(w, dict) and w.get('completeTradeTape') is False and type(w.get('evidenceComplete')) is bool)
        count, missing = w.get('observedTradeCount'), w.get('missingExecutionClockCount')
        need(integer(count) and count <= 4096 and integer(missing) and missing <= 4096)
        need(integer(w.get('observedSubscriptionDurationMs')) and w['observedSubscriptionDurationMs'] <= at and
             w.get('windowObservedSinceSubscription') is (w['observedSubscriptionDurationMs'] >= ms))
        need(isinstance(w.get('evidence'), list) and len(w['evidence']) <= 8 and len(w['evidence']) <= count)
        need(w['evidenceComplete'] == (len(w['evidence']) == count))
        evidence, ids = [], set()
        for r in w['evidence']:
            need(isinstance(r, dict) and set(r) == set(FLOW_FIELDS))
            need(r['ticker'] == ticker and r['streamEpoch'] == flow['streamEpoch'] and integer(r['sid']) and r['sid'] >= 1 and r['sid'] == flow.get('sid'))
            need(r['sequence'] is None or integer(r['sequence']))
            need(isinstance(r['tradeId'], str) and 0 < len(r['tradeId']) <= 128 and r['tradeId'] not in ids)
            ids.add(r['tradeId'])
            need(r['tradeId'] not in retained_evidence or retained_evidence[r['tradeId']] == r)
            retained_evidence[r['tradeId']] = r
            need(isinstance(r['bodySha256'], str) and bool(re.fullmatch(r'[0-9a-f]{64}', r['bodySha256'])))
            need(all(integer(r[k]) for k in ('sourceTsMs', 'receivedMs', 'availableMs')) and
                 r['receivedMs'] <= r['availableMs'] <= at and at-ms < r['sourceTsMs'] <= at and r['sourceTsMs']-r['receivedMs'] <= 1000)
            need(r['direction'] is None or type(r['direction']) is int and r['direction'] in (-1, 1))
            need(r['isBlockTrade'] is None or type(r['isBlockTrade']) is bool)
            need(decimal(r['quantity']) > 0 and decimal(r['yesPrice'])+decimal(r['noPrice']) == 1)
            evidence.append({k: r[k] for k in FLOW_FIELDS})
        need(isinstance(w.get('evidenceSha256'), str) and bool(re.fullmatch(r'[0-9a-f]{64}', w['evidenceSha256'])))
        if w['evidenceComplete']:
            original = json.dumps(evidence, separators=(',', ':'), ensure_ascii=False, allow_nan=False)
            need(hashlib.sha256(original.encode()).hexdigest() == w['evidenceSha256'])
            total = sum((decimal(r['quantity']) for r in evidence), Decimal(0))
            unknown = sum((decimal(r['quantity']) for r in evidence if r['direction'] is None), Decimal(0))
            known = sum((decimal(r['quantity'])*r['direction'] for r in evidence if r['direction'] is not None), Decimal(0))
            need(decimal(w['observedQuantity']) == total and decimal(w['unknownDirectionQuantity']) == unknown and signed(w['knownSignedQuantity']) == known)
            need(w.get('signedQuantity') is None if not evidence or unknown or missing else signed(w['signedQuantity']) == known)
            need(w.get('blockTradeCount') == sum(r['isBlockTrade'] is True for r in evidence) and
                 w.get('unknownBlockClassificationCount') == sum(r['isBlockTrade'] is None for r in evidence))
        else:
            # Full aggregates are original source reports only, never validated
            # from truncated evidence or promoted to model inputs below.
            need(abs(signed(w['knownSignedQuantity'])) <= decimal(w['observedQuantity']) and decimal(w['unknownDirectionQuantity']) <= decimal(w['observedQuantity']))
        need(w.get('signedQuantity') is None if count == 0 or missing or decimal(w['unknownDirectionQuantity']) > 0 else signed(w['signedQuantity']) == signed(w['knownSignedQuantity']))
        need(integer(w.get('blockTradeCount')) and w['blockTradeCount'] <= count and integer(w.get('unknownBlockClassificationCount')) and w['unknownBlockClassificationCount'] <= count)
        need(decimal(w['receiptOnlyActivityQuantity']) == 0 if missing == 0 else decimal(w['receiptOnlyActivityQuantity']) > 0)
    return json.loads(compact(flow))


class Tail:
    """Fresh suffix only. A checked suffix chain is not a full-history audit."""
    def __init__(self, path, from_start=False):
        flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
        self.fd = os.open(path, flags)
        info = os.fstat(self.fd)
        if not stat.S_ISREG(info.st_mode):
            raise ValueError("OBSERVER_NOT_REGULAR")
        self.identity = (info.st_dev, info.st_ino)
        self.path, self.position, self.buffer = Path(path), 0 if from_start else info.st_size, b""
        self.skip = bool(not from_start and info.st_size and os.pread(self.fd, 1, info.st_size - 1) != b"\n") if hasattr(os, "pread") else False
        if not from_start and info.st_size and not hasattr(os, "pread"):
            os.lseek(self.fd, info.st_size - 1, os.SEEK_SET)
            self.skip = os.read(self.fd, 1) != b"\n"
        os.lseek(self.fd, self.position, os.SEEK_SET)
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
            # Preserve original numeric bytes, including official JSON float
            # rendering. The producer serializes sorted root keys; remove its
            # final top-level hash field without re-encoding nested numbers.
            if list(row) != ["event", "hash", "previous", "sequence"] or not re.fullmatch(r"[0-9a-f]{64}", row.get("hash", "")):
                raise ValueError("OBSERVER_JOURNAL_FIELDS")
            field = b',"hash":"' + row["hash"].encode() + b'"'
            position = line.rfind(field)
            unsigned = line[:position] + line[position + len(field):]
            if position < 0 or hashlib.sha256(unsigned).hexdigest() != row.get("hash"):
                raise ValueError("OBSERVER_JOURNAL_HASH")
            if self.previous is not None and (row["previous"] != self.previous or row["sequence"] != self.sequence + 1):
                raise ValueError("OBSERVER_JOURNAL_GAP")
            self.previous, self.sequence = row["hash"], row["sequence"]
            yield row

    def close(self):
        os.close(self.fd)


TRADE_SAMPLING_POLICY = {
    "version": 1, "samplingMode": "first_observed_public_trade_v1",
    "maximumTriggerAgeMs": 2000, "maximumSigAgeMs": 3000,
    "maximumExternalAgeMs": 20000, "maximumSourceExportAgeMs": 2000,
    "minimumScopeSampleSpacingMs": 1000, "maximumSeenIdentities": 32768,
    "maximumSigScopes": 237, "maximumPendingPerScope": 128,
    "horizonsMs": [1000, 3000, 10000, 30000, 120000], "labelSlackMs": 3000,
    "requireKnownDirection": True, "consumeSkippedIdentity": True,
    "completeTradeTape": False, "resolutionEquivalenceVerified": False,
    "pricingAuthority": False, "executionAuthority": False,
}
TRADE_SAMPLING_POLICY_SHA256 = hashlib.sha256(compact(TRADE_SAMPLING_POLICY).encode()).hexdigest()


class Study:
    def __init__(self, emit, horizons=(1000, 3000, 10000, 30000, 120000), sig_age=3000, external_age=20000, label_slack=3000, sampling_mode="book"):
        if sampling_mode not in ("book", "trade"):
            raise ValueError("OBSERVER_SAMPLING_MODE")
        if sampling_mode == "trade" and (tuple(horizons) != tuple(TRADE_SAMPLING_POLICY["horizonsMs"]) or sig_age != 3000 or external_age != 20000 or label_slack != 3000):
            raise ValueError("OBSERVER_TRADE_FIXED_POLICY")
        self.sampling_mode = sampling_mode
        self.trigger_seen, self.sig_current = {}, {}
        self.trigger_disabled = False
        self.emit, self.horizons = emit, horizons
        self.sig_age, self.external_age, self.label_slack = sig_age, external_age, label_slack
        self.external, self.history = {}, collections.defaultdict(collections.deque)
        self.external_cache = collections.defaultdict(dict)
        self.external_invalid, self.external_quarantine = set(), set()
        self.external_tickers = set()
        self.external_market_scopes = set()
        self.source_histories = {}  # <=237 markets x2 source histories; old epochs discarded.
        self.external_flow, self.external_flow_seen = {}, {}
        self.pending = collections.defaultdict(collections.deque)
        self.last_sample, self.last_source, self.revisions = {}, {}, {}
        self.counts = collections.Counter()
        self.markets = set()

    def external_update(self, envelope, now):
        # Fetch completion is when this observer first has these features.
        source = envelope.get("source", "kalshi_official_v2")
        if source not in ("kalshi_official_v2", "kalshi_official_ws_v2"):
            raise ValueError("OBSERVER_EXTERNAL_SOURCE")
        ws = source == "kalshi_official_ws_v2"
        ws_status = envelope.get("websocket")
        ws_coverage = ws_status.get("coverage") if isinstance(ws_status, dict) else None
        ws_epoch = ws_coverage.get("connections") if ws and isinstance(ws_coverage, dict) else None
        if ws_epoch is not None and (type(ws_epoch) is not int or ws_epoch < 1):
            ws_epoch = None  # Absent/unusable epoch never proves continuity.
        for row in envelope["markets"]:
            for market in row["sigMarketIds"]:
                market = str(market)
                if market not in self.external_market_scopes:
                    if len(self.external_market_scopes) >= 237:
                        self.counts["externalScopeLimit"] += 1
                        continue
                    self.external_market_scopes.add(market)
                if ws:
                    self._flow_update(market, row, now, ws_epoch)
                if market not in self.external_cache[source] and len(self.external_cache[source]) >= 237:
                    self.counts["externalScopeLimit"] += 1
                    continue
                key = (source, market)
                ticker = row.get("ticker")
                if ticker not in self.external_tickers and len(self.external_tickers) >= 237:
                    self.counts["externalScopeLimit"] += 1
                    continue
                if isinstance(ticker, str):
                    self.external_tickers.add(ticker)
                if ws and row.get("valid") is not True:
                    self.external_invalid.add(key)
                    if row.get("invalidReason") not in ("stale", "source_clock_ahead"):
                        self._invalidate_external_history(market, source)
                    self.counts["externalWsWithdrawn"] += 1
                    self._select_external(market, now)
                    continue
                try:
                    received = millis(row["receivedAt"])
                    source_ts = row.get("sourceTsMs") if ws else None
                    if (received < 0 or received > now or now-received > self.external_age or
                            source_ts is not None and (type(source_ts) is not int or source_ts < 0 or source_ts > now or now-source_ts > self.external_age)):
                        self.counts["externalStaleOrFuture"] += 1
                        if ws:
                            self.external_invalid.add(key)
                        self._select_external(market, now)
                        continue
                    original_hash = row.get("stateChainSha256", row.get("bodySha256"))
                    if not isinstance(ticker, str) or not ticker:
                        raise ValueError("OBSERVER_EXTERNAL_TICKER")
                    if not isinstance(original_hash, str) or not re.fullmatch(r"[0-9a-f]{64}", original_hash):
                        raise ValueError("OBSERVER_EXTERNAL_PROVENANCE")
                    sid, seq = (row.get("sid"), row.get("sequence")) if ws else (None, None)
                    if ws and not (type(sid) is int and sid > 0 and type(seq) is int and seq >= 0):
                        raise ValueError("OBSERVER_EXTERNAL_CURSOR")
                    n = row["normalized"]
                    f = features(levels(n["yesBids"], False), levels(n["yesAsks"], True))
                    value = {"ticker": ticker, "mappingKind": row["mappingKind"], "receivedMs": received,
                             "availableMs": now, "bodySha256": original_hash, "features": f,
                             "source": source, "hashKind": "state_chain" if ws else "original_body", "pricingAuthority": False}
                    if ws:
                        value.update(sid=sid, sequence=seq, sourceTsMs=source_ts, wsEpoch=ws_epoch,
                                     streamBoundary=0)
                except (ValueError, InvalidOperation, KeyError, TypeError):
                    self.counts["externalInvalidOrOneSided"] += 1
                    self.external_invalid.add(key)
                    self._invalidate_external_history(market, source)
                    self._select_external(market, now)
                    continue
                prior = self.external_cache[source].get(market)
                if prior and received < prior["receivedMs"]:
                    self.counts["externalClockRegression"] += 1
                    continue
                same_epoch = prior and (not ws or ws_epoch == prior.get("wsEpoch"))
                if ws and prior:
                    value["streamBoundary"] = prior.get("streamBoundary", 0)
                    if same_epoch and sid == prior["sid"] and seq < prior["sequence"]:
                        if received <= prior["receivedMs"]:
                            self.counts["externalClockRegression"] += 1
                            continue
                        # The exporter validates actual snapshots/continuity.
                        # A later original receipt with reset sequence can be a
                        # collector restart reusing SID/epoch. Never splice it
                        # into the old innovation path or renew an old receipt.
                        value["streamBoundary"] += 1
                        self.counts["externalUnknownNewStreams"] += 1
                same_cursor = prior and (same_epoch and sid == prior.get("sid") and seq == prior.get("sequence") if ws else received == prior["receivedMs"])
                if same_cursor:
                    # Compare immutable observation, excluding first availability.
                    expected = {k: v for k, v in prior.items() if k != "availableMs"}
                    actual = {k: v for k, v in value.items() if k != "availableMs"}
                    if actual != expected:
                        self.external_quarantine.add(key)
                        self.external.pop(market, None)
                        self._invalidate_external_history(market, source)
                        self.counts["externalConflicts"] += 1
                        continue
                    self.counts["externalDuplicates"] += 1
                else:
                    self.external_cache[source][market] = value
                    self.external_quarantine.discard(key)
                    self._remember_external(market, value, now)
                self.external_invalid.discard(key)
                self._select_external(market, now)
        self.counts["externalPolls"] += 1
        if self.sampling_mode == "trade" and ws:
            self._trigger_trades(envelope, now)

    @staticmethod
    def _external_identity(value):
        return tuple(value.get(k) for k in ("source", "ticker", "mappingKind", "sid", "wsEpoch", "streamBoundary"))

    def _flow_update(self, market, row, now, epoch):
        if row.get('tradeFlow') is None:
            self.external_flow.pop(market, None)
            return
        try:
            flow = validate_trade_flow(row['tradeFlow'], row['ticker'], now, epoch)
            sha = hashlib.sha256(compact(flow).encode()).hexdigest()
            prior = self.external_flow_seen.get(market)
            if prior and flow['asOfMs'] < prior[0]:
                self.counts['tradeFlowClockRegression'] += 1
                return
            if prior and flow['asOfMs'] == prior[0]:
                if sha != prior[1]:
                    self.external_flow.pop(market, None)
                    self.counts['tradeFlowSnapshotConflict'] += 1
                return  # Original first availability never renewed.
            self.external_flow_seen[market] = (flow['asOfMs'], sha)
            self.external_flow[market] = {'sourceSnapshot': flow, 'sourceSnapshotSha256': sha, 'availableMs': now}
        except (ValueError, KeyError, TypeError, InvalidOperation):
            if self.sampling_mode == "trade":
                # Counter only: invalid untrusted evidence never becomes a trigger.
                raw = row.get('tradeFlow')
                windows = raw.get('windows') if isinstance(raw, dict) else None
                records = []
                if isinstance(windows, dict):
                    for window in list(windows.values())[:3]:
                        evidence = window.get('evidence') if isinstance(window, dict) else None
                        if isinstance(evidence, list):
                            records.extend(r for r in evidence[:8] if isinstance(r, dict))
                future_export = isinstance(raw, dict) and type(raw.get('asOfMs')) is int and raw['asOfMs'] > now
                if future_export or any(type(r.get(k)) is int and r[k] > now for r in records for k in ('sourceTsMs', 'receivedMs', 'availableMs')):
                    self.counts['tradeFutureSkip'] += 1
                self.counts['tradeInvalidEvidenceSkip'] += 1
            self.external_flow.pop(market, None)
            self.counts['tradeFlowInvalid'] += 1

    def _trigger_trades(self, envelope, now):
        if self.trigger_disabled:
            self.counts["tradeDisabledPollSkip"] += 1
            return
        for row in envelope["markets"]:
            for market in map(str, row["sigMarketIds"]):
                cached = self.external_flow.get(market)
                if not cached:
                    self.counts["tradeNoValidFlowSkip"] += 1
                    continue
                snapshot = cached["sourceSnapshot"]
                if snapshot["ticker"] != row["ticker"]:
                    self.counts["tradeScopeSkip"] += 1
                    continue
                records = {}
                for window in snapshot["windows"].values():
                    if not window["evidenceComplete"]:
                        self.counts["tradePartialWindowSkip"] += 1
                        continue
                    for record in window["evidence"]:
                        records[record["tradeId"]] = record
                for record in sorted(records.values(), key=lambda r: (r["availableMs"], r["sourceTsMs"], r["tradeId"])):
                    identity = (market, record["ticker"], record["tradeId"])
                    fingerprint = hashlib.sha256(compact(record).encode()).hexdigest()
                    prior = self.trigger_seen.get(identity)
                    if prior is not None:
                        if prior != fingerprint:
                            self.trigger_disabled = True
                            self.counts["tradeIdentityConflict"] += 1
                            return
                        self.counts["tradeDuplicateSkip"] += 1
                        continue
                    if len(self.trigger_seen) >= 32768:
                        self.trigger_disabled = True
                        self.counts["tradeIdentityCapReached"] += 1
                        return  # Never evict a seen ID and allow it to trigger again.
                    self.trigger_seen[identity] = fingerprint  # Consume even skipped IDs.
                    if row["mappingKind"] != "direct_party_reference":
                        self.counts["tradeAnalogSkip"] += 1
                        continue
                    if not (record["sourceTsMs"] <= now and record["receivedMs"] <= record["availableMs"] <= snapshot["asOfMs"] <= cached["availableMs"] <= now):
                        self.counts["tradeFutureSkip"] += 1
                        continue
                    if now-record["sourceTsMs"] > 2000 or now-record["receivedMs"] > 2000 or now-snapshot["asOfMs"] > 2000:
                        self.counts["tradeOldSkip"] += 1
                        continue
                    if record["direction"] is None:
                        self.counts["tradeUnsignedSkip"] += 1
                        continue
                    ext = self._select_external(market, now)
                    if not ext or ext["ticker"] != record["ticker"] or ext["mappingKind"] != "direct_party_reference":
                        self.counts["tradeNoCurrentExternalSkip"] += 1
                        continue
                    books = [(key, value) for key, value in self.sig_current.items() if key[0] == market]
                    if not books:
                        self.counts["tradeNoCurrentSigSkip"] += 1
                        continue
                    for key, book in books:
                        b = book["row"]["event"]["book"]
                        if not (0 <= now-book["observedMs"] and 0 <= now-millis(b["sourceAt"]) <= 3000 and 0 <= now-millis(b["receivedAt"]) <= 3000):
                            self.counts["tradeStaleSigSkip"] += 1
                            continue
                        if now-self.last_sample.get(key, -10000) < 1000:
                            self.counts["tradeSampleSpacingSkip"] += 1
                            continue
                        if len(self.pending[key]) >= 128:
                            self.counts["tradePendingCapSkip"] += 1
                            continue
                        trigger = {"kind": "first_observed_public_trade_v1", "record": json.loads(compact(record)),
                                   "sourceSnapshotSha256": cached["sourceSnapshotSha256"], "sourceExportMs": snapshot["asOfMs"],
                                   "observerFirstAvailableMs": cached["availableMs"], "firstObservedAtMs": now,
                                   "sigBookObservedMs": book["observedMs"], "resolutionEquivalenceVerified": False}
                        if self._sample(book["row"], now, book["features"], trigger):
                            self.counts["tradeTriggeredSamples"] += 1

    def _flow_for_decision(self, market, now):
        cached = self.external_flow.get(market)
        if not cached or cached['availableMs'] > now or not 0 <= now-cached['sourceSnapshot']['asOfMs'] <= 2000:
            return None
        inputs = {}
        for text_ms, w in cached['sourceSnapshot']['windows'].items():
            rows = [r for r in w['evidence'] if now-int(text_ms) < r['sourceTsMs'] <= now]
            identified = w['evidenceComplete'] and w['missingExecutionClockCount'] == 0 and rows and all(r['direction'] is not None for r in rows)
            inputs[text_ms] = {'observedSignedQuantity': str(sum((decimal(r['quantity'])*r['direction'] for r in rows), Decimal(0))) if identified else None,
                               'validatedEvidenceComplete': w['evidenceComplete'], 'observedTradeCount': len(rows) if w['evidenceComplete'] else None,
                               'completeTradeTape': False}
        return {**cached, 'decisionMs': now, 'snapshotAgeMs': now-cached['sourceSnapshot']['asOfMs'],
                'inputs': inputs, 'pricingAuthority': False, 'executionAuthority': False}

    def _invalidate_external_history(self, market, source):
        entry = self.source_histories.get((market, source))
        if entry:
            entry[1].clear()

    def _remember_external(self, market, value, now):
        key, identity = (market, value["source"]), self._external_identity(value)
        entry = self.source_histories.get(key)
        if entry is None or entry[0] != identity:
            entry = (identity, collections.deque())
            self.source_histories[key] = entry
        h = entry[1]
        h.append(value)
        while h and (h[0]["receivedMs"] < now-180000 or len(h) > 4096):
            h.popleft()

    def _select_external(self, market, now):
        selected = None
        for source in ("kalshi_official_ws_v2", "kalshi_official_v2"):
            value = self.external_cache[source].get(market)
            key = (source, market)
            if (value and key not in self.external_invalid and key not in self.external_quarantine and
                    0 <= now-value["receivedMs"] <= self.external_age and value["availableMs"] <= now and
                    (value.get("sourceTsMs") is None or 0 <= now-value["sourceTsMs"] <= self.external_age)):
                selected = value
                break
        prior = self.external.get(market)
        if selected is None:
            self.external.pop(market, None)
            self.history[market] = collections.deque()
            return None
        if selected is prior:
            return selected
        if prior and (prior["source"], prior.get("sid"), prior.get("wsEpoch"), prior.get("streamBoundary")) != (selected["source"], selected.get("sid"), selected.get("wsEpoch"), selected.get("streamBoundary")):
            self.counts["externalSourceChanges"] += 1
        self.external[market] = selected
        self.history[market] = self.source_histories.get((market, selected["source"]), (None, collections.deque()))[1]
        return selected

    def breadth(self, row, now):
        frame = row["event"]
        if frame.get("source") != "official_sig_realtime":
            raise ValueError("OBSERVER_BREADTH_SOURCE")
        if frame.get("event") == "stream_reset":
            self.accept({"event": {"type": "gap"}}, now)
            self.revisions.clear()
            return
        if frame.get("event") != "markets_batch":
            return
        received = frame["receivedAt"]
        for scope in frame["payload"]["markets"]:
            market, batch = str(scope["marketId"]), scope["batch"]
            delivery = batch["delivery"]
            revision, previous = delivery["revision"], delivery["previousRevision"]
            if delivery["model"] != "best-effort-authoritative-resync" or revision <= previous:
                raise ValueError("OBSERVER_BREADTH_REVISION")
            old = self.revisions.get(market)
            if old is not None and revision <= old:
                continue
            if (old is not None and previous != old) or batch.get("resyncRequired") or batch.get("marketSettled"):
                self.accept({"event": {"type": "gap", "marketId": market}}, now)
            self.revisions[market] = revision
            for b in batch.get("books", []):
                def exact(rows):
                    result = []
                    for level in rows:
                        p = Decimal(str(level["price"]))
                        tick = Decimal("0.005000")
                        canonical = (p / tick).quantize(Decimal(1)) * tick
                        if abs(p - canonical) > Decimal("0.000000000001"):
                            raise ValueError("OBSERVER_OFFICIAL_TICK")
                        result.append({"price": format(canonical, ".6f"), "quantity": level["quantity"]})
                    return result
                self.accept({"hash": row["hash"], "sequence": row["sequence"], "event": {"type": "book", "book": {
                    "marketId": market, "exchangeId": str(b["exchangeId"]), "sequence": str(b["asOf"]["sequence"]),
                    "sourceAt": b["asOf"]["at"], "receivedAt": received,
                    "bids": exact(b["bids"]), "asks": exact(b["asks"])}}}, now)

    def accept(self, row, now):
        event = row["event"]
        if event.get("type") == "gap" or (event.get("type") == "coverage" and event.get("complete") is not True):
            # Incomplete scope coverage invalidates that scope's horizon path.
            # A complete trade-tape coverage receipt is not a global stream reset.
            self.counts["coverageBoundaries"] += 1
            if self.sampling_mode == "trade":
                for key in list(self.sig_current):
                    if (not event.get("marketId") or key[0] == str(event["marketId"])) and (not event.get("exchangeId") or key[1] == str(event["exchangeId"])):
                        self.sig_current.pop(key)
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
            if self.sampling_mode == "trade":
                self.sig_current.pop(key, None)
            self.counts["sigFuture"] += 1
            return
        if self.sampling_mode == "trade" and b.get("invalid") is True:
            self.accept({"event": {"type": "gap", "marketId": market, "exchangeId": exchange}}, now)
            self.counts["tradeInvalidSigSkip"] += 1
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
            if self.sampling_mode == "trade":
                self.sig_current.pop(key, None)
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
                if "samplingPolicySha256" in sample:
                    label.update(samplingPolicySha256=sample["samplingPolicySha256"], samplingMode="first_observed_public_trade_v1")
                self.emit(label)
                self.counts["labels"] += 1
            if not sample["remaining"] or now > sample["decisionMs"] + max(self.horizons) + self.label_slack:
                waiting.remove(sample)
        if self.sampling_mode == "trade":
            if key in self.sig_current or len(self.sig_current) < 237:
                self.sig_current[key] = {"row": json.loads(compact(row)), "features": f, "observedMs": now}
            else:
                self.counts["tradeSigScopeCapSkip"] += 1
            return  # Actual SIG books still supply future labels, never triggers.
        self._sample(row, now, f)

    def _sample(self, row, now, f, trigger=None):
        b = row["event"]["book"]
        market, exchange = str(b["marketId"]), str(b["exchangeId"])
        source, received = millis(b["sourceAt"]), millis(b["receivedAt"])
        key, waiting = (market, exchange), self.pending[(market, exchange)]
        # Samples are per-market at most once/second, not every journal update.
        if now - received > self.sig_age or now - source > self.sig_age or now - self.last_sample.get(key, -10000) < 1000:
            return
        ext = self._select_external(market, now)
        if not ext or now - ext["receivedMs"] > self.external_age or ext["availableMs"] > now:
            self.counts["noCurrentExternal"] += 1
            return
        sample_id = hashlib.sha256(f"{market}:{exchange}:{row['hash']}:{ext['bodySha256']}:{now}".encode()).hexdigest()
        changes, anchors = {}, {}
        for ms in (1000, 5000, 30000):
            prior = next((v for v in reversed(self.history[market]) if v["receivedMs"] <= now-ms and v["availableMs"] <= now), None)
            eligible = prior is not None and now-ms-prior["receivedMs"] <= self.external_age
            changes[str(ms)] = (logit(ext["features"]["midpoint"]) - logit(prior["features"]["midpoint"])) if eligible else None
            anchors[str(ms)] = ({**{k: prior.get(k) for k in ("bodySha256", "receivedMs", "availableMs", "source", "ticker", "mappingKind", "sid", "sequence", "wsEpoch", "streamBoundary", "sourceTsMs")}, "midpoint": prior["features"]["midpoint"]} if eligible else None)
        sample = {"kind": "decision_features", "sampleId": sample_id, "decisionMs": now, "decisionAt": stamp(now),
                  "marketId": market, "exchangeId": exchange, "sig": {"journalHash": row["hash"], "journalSequence": row["sequence"],
                  "sourceAt": b["sourceAt"], "receivedAt": b["receivedAt"], "features": f},
                  "external": ext, "externalLogitChanges": changes, "externalLagAnchors": anchors, "pricingAuthority": False,
                  "externalTradeFlow": self._flow_for_decision(market, now),
                  "executionAuthority": False, "horizonsMs": list(self.horizons)}
        if trigger is not None:
            sample["samplingPolicy"] = json.loads(compact(TRADE_SAMPLING_POLICY))
            sample["samplingPolicySha256"] = TRADE_SAMPLING_POLICY_SHA256
            sample["trigger"] = trigger
            sample["sampleId"] = hashlib.sha256(compact({"policy": TRADE_SAMPLING_POLICY_SHA256, "trigger": trigger, "bookSampleId": sample_id}).encode()).hexdigest()
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
        return True


def require_trade_output_fresh(path):
    # Seen identities are process-local. Never append a restarted process to its
    # prior trade cohort and silently label reobserved IDs as new information.
    if path.exists() and ((path / "status.json").exists() or (path / "sampling-policy.json").exists() or any(path.glob("*.jsonl.gz"))):
        raise ValueError("OBSERVER_TRADE_NEW_COHORT_REQUIRED")


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
    parser.add_argument("--format", choices=["native", "breadth"], default="native")
    parser.add_argument("--sampling-mode", choices=["book", "trade"], default="book")
    args = parser.parse_args()
    if args.sampling_mode == "trade":
        require_trade_output_fresh(args.output)
    # Unix VPS read-only sidecar, advisory lock kept on the same inode.
    import fcntl
    args.output.mkdir(parents=True, exist_ok=True, mode=0o700)
    lock = (args.output / "observer.lock").open("a+")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    if args.sampling_mode == "trade":
        # A crash before the first sample/status still permanently marks this
        # cohort as started. Restart requires another explicitly chosen output.
        with (args.output / "sampling-policy.json").open("x", encoding="utf-8") as marker:
            marker.write(compact({"samplingPolicy": TRADE_SAMPLING_POLICY, "samplingPolicySha256": TRADE_SAMPLING_POLICY_SHA256, "observerStartedAtMs": int(time.time()*1000)}) + "\n")
            marker.flush(); os.fsync(marker.fileno())
        directory = os.open(args.output, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    base = args.journal
    segment = 0
    if args.format == "breadth":
        while base.with_name(f"{base.stem}-part-{segment + 1:04}.jsonl").exists():
            segment += 1
    current = base.with_name(f"{base.stem}-part-{segment:04}.jsonl") if segment else base
    tail, output = Tail(current), Output(args.output)
    study = Study(output.emit, sampling_mode=args.sampling_mode)
    started, last_poll, last_status = time.monotonic(), 0, 0
    stopping = [False]
    def stop(_signal, _frame):
        stopping[0] = True
    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)
    try:
        while not stopping[0] and not (args.output / "STOP").exists():
            if args.duration_seconds and time.monotonic() - started >= args.duration_seconds:
                break
            if time.monotonic() - last_poll >= 1:
                last_poll = time.monotonic()
                try:
                    for route in ["/v1/quotes", "/v1/ws/quotes"]:
                        request = urllib.request.Request("http://127.0.0.1:17841" + route)
                        with urllib.request.urlopen(request, timeout=2) as response:
                            raw = response.read(16 * 1024 * 1024 + 1)
                        if len(raw) > 16 * 1024 * 1024:
                            raise ValueError("OBSERVER_CACHE_LIMIT")
                        study.external_update(json.loads(raw), int(time.time() * 1000))
                except (OSError, TimeoutError):
                    study.counts["externalUnavailable"] += 1
            for row in tail.read():
                (study.breadth if args.format == "breadth" else study.accept)(row, int(time.time() * 1000))
            if args.format == "breadth" and not tail.buffer:
                next_file = base.with_name(f"{base.stem}-part-{segment + 1:04}.jsonl")
                if next_file.exists():
                    tail.close()
                    segment += 1
                    tail = Tail(next_file, from_start=True)
            if time.monotonic() - last_status >= 5:
                last_status = time.monotonic()
                status = {"at": stamp(int(time.time() * 1000)), "state": "observing", "pid": os.getpid(),
                          "counters": dict(study.counts), "observedMarkets": len(study.markets),
                          "pendingSamples": sum(map(len, study.pending.values())), "journalPosition": tail.position,
                          "journalSequence": tail.sequence, "originalJournalHash": tail.previous, "format": args.format, "captureSegment": segment,
                          "readOnly": True, "executionAuthority": False, "historyPrefixVerified": False}
                if args.sampling_mode == "trade":
                    status.update(samplingPolicy=TRADE_SAMPLING_POLICY, samplingPolicySha256=TRADE_SAMPLING_POLICY_SHA256, triggerDisabled=study.trigger_disabled)
                temp = args.output / "status.json.tmp"
                temp.write_text(compact(status) + "\n", encoding="utf-8")
                os.replace(temp, args.output / "status.json")
            time.sleep(.1)
        status_file = args.output / "status.json"
        status = json.loads(status_file.read_text()) if status_file.exists() else {"pid": os.getpid()}
        status.update(state="stopped", at=stamp(int(time.time() * 1000)))
        temp = args.output / "status.json.tmp"
        temp.write_text(compact(status) + "\n", encoding="utf-8")
        os.replace(temp, status_file)
    finally:
        output.close()
        tail.close()
        lock.close()


if __name__ == "__main__":
    main()
