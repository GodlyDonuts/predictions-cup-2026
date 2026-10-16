# Kalshi WebSocket shadow feed — October 16

Integrated read-only shadow feed. At10:08:15UTC the owner's sole collector had237/237valid snapshots,191deltas,41tickers and one trade on one connection, with zero sequence gaps/invalid frames. Earlier live diagnostics corrected multi-subscription SID behavior and a small venue/local clock offset. External references remain conditional evidence, not reviewed probabilities or verified SIG settlement equivalence. SIG production continued independently.

## Primary protocol evidence

Observed **2026-10-16 09:45 UTC** from current official documentation:

- [Overview](https://docs.kalshi.com/websockets): production host `wss://external-api-ws.kalshi.com/trade-api/ws/v2`.
- [Authenticated quick start](https://docs.kalshi.com/getting_started/quick_start_websockets): Ed25519 signs `timestamp + GET + /trade-api/ws/v2`. Public ticker/trade channels still require an authenticated connection.
- [Orderbooks](https://docs.kalshi.com/websockets/orderbook-updates): snapshots precede additive deltas. Fixed-point fields are `yes_dollars_fp`, `no_dollars_fp`, `price_dollars`, `delta_fp`. Omitted snapshot sides contain no levels.
- [Commands](https://docs.kalshi.com/websockets/websocket-connection): subscribe confirmations assign SID; updates/unsubscribe address SID.
- [Keep alive](https://docs.kalshi.com/websockets/connection-keep-alive): server Ping approximately every ten seconds requires Pong. Pinned `ws` source defaults `autoPong:true`, explicitly requested here.
- [Trades](https://docs.kalshi.com/websockets/public-trades) and [ticker](https://docs.kalshi.com/websockets/market-ticker): original timestamps are retained; neither supplies executable depth or proves SIG resolution equivalence.

Original [AsyncAPI](https://docs.kalshi.com/asyncapi.yaml) SHA256 `3c86bae7d89b44baba1119b3a097a54054c1d851e1dcd8ad894c7cb35b9890b6`, retained privately at `Temp/moloch-kalshi-websocket-primary-20261016/asyncapi.yaml`. The original schema includes `use_yes_price`; book subscriptions explicitly pin it to false to preserve NO-leg price interpretation. Sequential consistency is enforced per SID. The official schema describes sequential snapshot/delta consistency and channel SIDs, but does not explicitly state the sequence reset domain. Per-SID handling is an implementation assumption supported by the current original live observations, not independently proved end-to-end delivery completeness.

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

One full verified237-ticker subscription is used per public orderbook/ticker/trade channel. The original schema has no100-ticker array maximum. Splitting a channel into chunks caused the actual server to return snapshots beyond the first chunk on the same SID; the initial live failure is retained as evidence. Initial snapshot sequence can be any positive integer. Deltas require a valid market snapshot and exact next SID sequence. Identical duplicate bytes do not renew book time. Gap/conflicting sequence invalidates **all** books on that SID, unsubscribes and resubscribes; late retired-SID frames cannot restore continuity. Other SID channels remain separate. Malformed public frames are retained with an allowlisted validation code before invalidation; auth headers/errors are excluded.

Integer millionths preserve fractional quantity and exact complements. Duplicate levels, overprecision, negative remaining depth, changed market IDs, foreign markets and crossed books fail closed. Complete original frame bytes, SHA256, send/event timestamps and actual receipt time remain distinct; reconstructed books retain snapshot and latest-frame hashes. No missing timestamp or complete trade tape is fabricated. Actual venue timestamps were up to10ms ahead at10:08. A configurable maximum1000ms skew is retained without clamping; a book remains invalid until local time catches up to its original source timestamp. Larger offsets still reject the frame.

Default stale interval is30seconds from original receipt and source time when present. Ticker/heartbeat activity cannot renew depth. A separate snapshotRefreshAfterMs defaults to30seconds, matching the previously stable scheduling baseline; public view validity also stays30seconds. The explicit10-second option remains an UNQUALIFIED trial only, not the published default. Only the book SID is requested (onefull237tickercommand). EachSID has at mostonependingrefresh and minimum10-second request spacing; actual snapshot or contiguous delta updates discharge corresponding pending tickers. A missing response does not renew any clock, triggers no duplicate outstanding requests, and views/observer eligibility expire normally. After30seconds with pending tickers, that one SID is explicitly unsubscribed/resubscribed, all its old books structurally invalidated, and fresh actual snapshots are required. This bounded timeout prevents one quiet/closed ticker from permanently suppressing all237book refreshes; repeated timer checks cannot resubscribe the retired SID again. Actual source/receipt clocks remain unchanged until genuine data arrives. Stale books become invalid to public consumers independently of this request schedule. Thirty seconds of socket silence invalidates the connection. Reconnect uses1/2/4/.../60-second delays and fresh state/snapshots, with a configurable total ceiling(default1000), without resetting after brief successful sessions. Inbound processing is serialized and bounded to1024queued frames and64MiB. Exported views slice20levels before cloning; full internal books remain retained. STOP closes only this socket.

## Verification and limits

`node --test moloch/external-data/kalshi-websocket.test.mjs`: **18/18 offline checks pass** using generated test keys and injected sockets. Tests also reproduce the actual first-SID/index130 failure, bounded future clock quarantine, independent byte pressure and rejected-frame provenance. These checks do not establish alpha.

Collector option `--websocket true` uses the existing in-memory key and same original journal. `GET /v1/ws/quotes` serves reconstructed books with validity and mapping classes; `GET /v1/status` exposes actual coverage/counters. REST cache never repairs a missing WS sequence implicitly. Prospective model qualification and economic results remain unproved. Read-only feed availability grants no pricing or trading authority.

## Refresh trial and diagnostic boundary

Root reported the v6 explicit10-second refresh trial produced5connections in80seconds with KALSHI_WS_SOCKET, while prior v5 with30-second scheduling had run57minutes on one connection. These operational observations do not prove the10-second schedule caused the errors. The published default is therefore restored to30seconds pending a controlled diagnostic trial. Setting snapshotRefreshAfterMs:10000 explicitly opts into the unqualified experiment. Sanitized socketDiagnostic exposes only fixed allowlisted standard error codes, numeric close codes and connection number; raw error messages, headers, close reasons and credentials are never retained. Observer20-second eligibility may still fall back during quiet books under30-second scheduling: no freshness window is relaxed and no REST/WS innovation histories are mixed to conceal that gap.
