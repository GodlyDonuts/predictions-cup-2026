# Kalshi WebSocket shadow feed — October 16

Source-only implementation. No live connection, account request, credential-file read or trading action was performed. External references are conditional evidence, not reviewed probabilities or verified equivalence to SIG settlement rules.

## Primary protocol evidence

Observed **2026-10-16 09:45 UTC** from current official documentation:

- [Overview](https://docs.kalshi.com/websockets): production host `wss://external-api-ws.kalshi.com/trade-api/ws/v2`.
- [Authenticated quick start](https://docs.kalshi.com/getting_started/quick_start_websockets): Ed25519 signs `timestamp + GET + /trade-api/ws/v2`. Public ticker/trade channels still require an authenticated connection.
- [Orderbooks](https://docs.kalshi.com/websockets/orderbook-updates): snapshots precede additive deltas. Fixed-point fields are `yes_dollars_fp`, `no_dollars_fp`, `price_dollars`, `delta_fp`. Omitted snapshot sides contain no levels.
- [Commands](https://docs.kalshi.com/websockets/websocket-connection): subscribe confirmations assign SID; updates/unsubscribe address SID.
- [Keep alive](https://docs.kalshi.com/websockets/connection-keep-alive): server Ping approximately every ten seconds requires Pong. Pinned `ws` source defaults `autoPong:true`, explicitly requested here.
- [Trades](https://docs.kalshi.com/websockets/public-trades) and [ticker](https://docs.kalshi.com/websockets/market-ticker): original timestamps are retained; neither supplies executable depth or proves SIG resolution equivalence.

Original [AsyncAPI](https://docs.kalshi.com/asyncapi.yaml) SHA256 `3c86bae7d89b44baba1119b3a097a54054c1d851e1dcd8ad894c7cb35b9890b6`, retained privately at `Temp/moloch-kalshi-websocket-primary-20261016/asyncapi.yaml`. Its current schema uses side-specific prices and does not include `use_yes_price`; no retired flag is invented. Sequential consistency is checked conservatively per SID, rather than assuming each market can reset sequence independently.

## Integration

Install this directory's lock with `npm ci --ignore-scripts`. Exact `ws` **8.22.0** and SHA512 integrity were verified against the [official registry](https://registry.npmjs.org/ws/8.22.0); the local test installation independently checked integrity before extraction. Project-root dependencies are untouched.

```js
import { ShadowFeed } from './kalshi-websocket.mjs';
const feed = new ShadowFeed({
  auth, // existing in-memory {id,key}; do not load another credential copy
  tickers: states.map(s => s.ticker),
  onReceipt: async (receipt, state) => {
    await existingJournal.append({kind:'websocket', ...receipt});
    // state.view(ticker, Date.now()); require valid === true.
  },
  onStatus: async status => persistPrivateStatus(status),
});
const task = feed.run();
// Existing collector STOP/finally: feed.stop(); await task.
```

The default factory uses standard authenticated `ws`. Injectable `transportFactory(url, options)` must return its Node event interface and honor fixed origin, redirect refusal, payload/handshake limits and automatic Pong. This module creates no REST queue, order capability, credential loader or process manager.

At most 100 tickers share each subscription group, with separate orderbook/ticker/trade SIDs. Initial snapshot sequence can be any positive integer. Deltas require a valid market snapshot and exact next SID sequence. Identical duplicate bytes do not renew book time. Gap/conflicting sequence invalidates **all** books on that SID, unsubscribes and resubscribes; late retired-SID frames cannot restore continuity. Other SID channels remain separate. Malformed frames or failed sinks invalidate connection books.

Integer millionths preserve fractional quantity and exact complements. Duplicate levels, overprecision, negative remaining depth, changed market IDs, foreign markets and crossed books fail closed. Complete original frame bytes, SHA256, send/event timestamps and actual receipt time remain distinct; reconstructed books retain snapshot and latest-frame hashes. No missing timestamp or complete trade tape is fabricated.

Default stale interval is 30 seconds from original receipt and source time when present. Ticker/heartbeat activity cannot renew depth. Stale books become invalid and request a real snapshot. Thirty seconds of socket silence invalidates the connection. Reconnect uses 1/2/4/.../60-second delays and fresh state/snapshots, with a configurable total ceiling (default 1,000), without resetting after brief successful sessions. Inbound processing is serialized and bounded to 1,024 queued frames. STOP closes only this socket.

## Verification and limits

`node --test moloch/external-data/kalshi-websocket.test.mjs`: **12/12 offline checks pass** using generated test keys and injected sockets. Tests cover 237-market batching, fractional depth/deletion, multi-market SID sequence, gap recovery/late old frames, duplicate clocks, invalid scope/price/source clocks, stale snapshots, original trades, lifecycle, reconnect ceiling and heartbeat expiry.

Parent collector wiring and actual connection evidence remain separate. REST cache must not renew or replace missing WS continuity. Prospective latency, semantic mapping, model qualification and realized economic results remain unproved. Paper 6's shadow-market recommendation motivates the feed; it grants no pricing or trading authority.
