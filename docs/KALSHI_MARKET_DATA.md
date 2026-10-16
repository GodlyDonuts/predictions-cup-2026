# Reading Kalshi election market data

Verified on October 16, 2026, approximately 09:00–09:02 UTC. This is a bounded, read-only REST connectivity test, not a continuously running feed or a SIG opportunity assessment. No SIG requests, orders, portfolio reads, or production changes were made. Ed25519 authentication succeeded on `GET /account/limits` (HTTP 200); successful public market requests alone would not prove authentication.

## Local credentials

The repository-root `.env` is ignored by Git and contains:

```dotenv
KALSHI_API_KEY_ID=<your key ID>
KALSHI_PRIVATE_KEY_PATH=C:/absolute/private/location/kalshi-market-data-ed25519.pem
KALSHI_KEY_TYPE=Ed25519
```

The current device stores the PEM under `%LOCALAPPDATA%/Moloch/secrets/kalshi-market-data-ed25519.pem`, outside this checkout, with user-only file permissions. Other devices need their own approved credential installation; this Markdown does not distribute credentials. Use read-only scope. Never print the PEM, authentication headers, signatures or `.env`, or commit them. The key ID and private key are separate values.

## Reproduce with Node.js

Tested with Node 24.19.0, built-in `fetch`, `node:fs` and `node:crypto`; no dependency installation required. Save the following as a private local `.mjs` file and run `node ABSOLUTE_SCRIPT_PATH` from the repository root. It uses the existing simple unquoted `.env` format, not a general dotenv parser. It only permits known read paths and sends credentials to the fixed official HTTPS origin. It performs one request per invocation, with no automatic retries.

```javascript
import fs from 'node:fs';
import crypto from 'node:crypto';

const env = Object.fromEntries(
  fs.readFileSync('.env', 'utf8').split(/\r?\n/)
    .filter(line => line.trim() && !line.startsWith('#'))
    .map(line => {
      const i = line.indexOf('=');
      if (i < 1) throw Error('Invalid local environment entry');
      return [line.slice(0, i), line.slice(i + 1)];
    })
);
const key = crypto.createPrivateKey(fs.readFileSync(env.KALSHI_PRIVATE_KEY_PATH));
if (key.asymmetricKeyType !== 'ed25519') throw Error('Expected Ed25519');
if (!env.KALSHI_API_KEY_ID) throw Error('Missing Kalshi key ID');

// Change only this suffix to one of the documented GET requests below.
const suffix = '/markets/SENATEIA-26-D/orderbook?depth=5';
if (!/^\/(account\/limits|events|markets(?:\/[A-Za-z0-9_-]+(?:\/orderbook)?)?)(\?|$)/.test(suffix)) {
  throw Error('Path outside read-only probe scope');
}
const path = '/trade-api/v2' + suffix;
const timestamp = String(Date.now());
// Sign timestamp + uppercase method + full path WITHOUT query parameters.
// Ed25519 signs these bytes directly: do not SHA256 them first or use RSA-PSS.
const signature = crypto.sign(
  null, Buffer.from(timestamp + 'GET' + path.split('?')[0], 'utf8'), key
).toString('base64');
const sentAt = new Date().toISOString();
const response = await fetch('https://external-api.kalshi.com' + path, {
  method: 'GET', redirect: 'error', signal: AbortSignal.timeout(20000),
  headers: {
    Accept: 'application/json',
    'KALSHI-ACCESS-KEY': env.KALSHI_API_KEY_ID,
    'KALSHI-ACCESS-TIMESTAMP': timestamp,
    'KALSHI-ACCESS-SIGNATURE': signature,
  },
});
const text = await response.text();
const receivedAt = new Date().toISOString();
if (!response.ok) {
  // Do not dump request headers or arbitrary error objects.
  throw Error(`HTTP ${response.status}; Retry-After=${response.headers.get('retry-after')}; stop and investigate`);
}
console.log(JSON.stringify({sentAt, receivedAt, path, status: response.status,
  body: JSON.parse(text)}, null, 2));
```

The exact copyable example above passed Node syntax checking and returned HTTP 200 for Iowa's order book at 09:03:18.961 UTC on October 16. It is not a production collector. For a repeated collector, serialize requests, persist Retry-After/backoff, prevent concurrent workers from independently consuming the same Kalshi account budget, and archive original responses with request/receipt clocks. Never log authentication headers.

## Tested requests and market selection

All paths below are relative to `https://external-api.kalshi.com/trade-api/v2`.

| Request | Actual result |
| --- | --- |
| `/account/limits` | 200; authenticated `basic` tier. Read bucket capacity 600, refill rate 200; write bucket capacity/refill rate 100. These are reported token-bucket fields, not a measured request throughput or a grant of write permission. |
| `/events?series_ticker=SENATERI&with_nested_markets=true&limit=20` | 200; `SENATERI-26` with Democratic and Republican markets; empty cursor. |
| `/markets/SENATERI-26-D` | 200; exact Rhode Island Democratic contract, rules and quote fields. |
| `/markets/SENATERI-26-D/orderbook?depth=5` | 200; five YES bid levels and five NO bid levels in `orderbook_fp`. |
| `/events?series_ticker=SENATEIA&with_nested_markets=true&limit=20` | 200; both 2026 AND 2028 Iowa Senate events. Filter exact event year! |
| `/events?series_ticker=SENATEKS&with_nested_markets=true&limit=20` | 200; both 2026 AND 2028 Kansas Senate events. |
| `/markets/GOVPARTYMI-26-D` | 200; Michigan Democratic governor contract. |
| Series `SENATEPARTYIA` and `SENATEPARTYKS` | 200 but empty events. Guessed names did not locate the races. |
| `/markets/HOUSECA48-26-D` | 404. This guessed House ticker is not a verified identifier. |

For Iowa and Kansas retain event tickers `SENATEIA-26` and `SENATEKS-26`, then returned market tickers `SENATEIA-26-D`, `SENATEIA-26-R`, `SENATEKS-26-D`, `SENATEKS-26-R`. Do not infer the year from a generic title. Follow every returned nonempty cursor using the same query plus `cursor`; an initial page is not exhaustive coverage.

For broader House/Senate/governor discovery use documented `/series`, then `/events?series_ticker=RETURNED_SERIES&with_nested_markets=true&limit=200`, and validate returned rules and event year. Series and tickers must come from returned catalog records or previously verified records, not naming guesses. The retained October 2 inventory and matching screen are starting points, not today's complete mappings:

- `research/public_reference_2026-10-02/coverage_report.md`: 993 archived party contracts / 496 races.
- `research/crossvenue_screen_2026-10-02/top10_candidates.json`: exact historical SIG IDs and Kalshi ticker candidates.
- `research/crossvenue_screen_2026-10-02/report.md`: 226 conditional matches from the old 237-market SIG catalog.

## Actual observed examples

These are historical receipts, already stale after observation; they are not fair probabilities or current execution recommendations.

| Kalshi contract | Receipt UTC, October 16 | YES bid | YES ask | Historical SIG candidate |
| --- | --- | --- | --- | --- |
| `SENATEIA-26-D` | 09:02:03.138 | 0.4300 | 0.4400 | market260 / exchange949 |
| `SENATEKS-26-D` | 09:02:04.219 | 0.3500 | 0.3600 | market356 / exchange1045 |
| `GOVPARTYMI-26-D` | 09:02:05.326 | 0.9310 | 0.9370 | market305; refresh exact exchange/contract |

Rhode Island's tested market response reported YES bid `0.9850` for `11.00` shares and YES ask `0.9940` for just `0.01` share. The subsequent book independently showed best YES bid `["0.9850","11.00"]` and best NO bid `["0.0060","0.01"]`, implying a YES ask of `0.9940` for only `0.01` share. This is why price alone is insufficient for useful sizing.

## Field interpretation and joining to SIG

- Current observed quote fields are decimal strings: `yes_bid_dollars`, `yes_ask_dollars`, `no_bid_dollars`, `no_ask_dollars`, and quantities such as `yes_bid_size_fp`. Do not divide dollar fields by 100. Do not truncate fractional shares.
- Current book response is `orderbook_fp.yes_dollars` and `orderbook_fp.no_dollars`. Both arrays are BID levels `[price, quantity]`, not bid/ask arrays. Find the maximum numeric bid; do not assume the first array item is best. Derive YES asks from `1 - NO bid`, and NO asks from `1 - YES bid`, preserving that level's quantity. Use exact decimal arithmetic; never floating point for accounting.
- A missing side means unavailable liquidity. A requested `depth=5` gives bounded depth, not the full available book. A last trade price is not an executable quote.
- Retain sent/received UTC and original source fields separately. `updated_time` is not proof that the quoted order was freshly placed. Independently requested event, market and book responses are not simultaneous.
- Match exact office, state, district, year, regular/special election, party/candidate and full payout rules. RI's returned rule specifies a Democratic representative sworn in for the term beginning in 2027, with accelerated-determination language. Do not assume that is identical to SIG's resolver.
- Refresh SIG through its existing authorized worker and shared limiter. This test did not create a second SIG collector or obtain simultaneous SIG depth, so no current cross-market edge is established.

## Streaming and limits of this verification

REST authentication and the GETs listed above were tested. WebSocket connectivity was **not** tested in this task. For a later authenticated streaming test, official docs specify `wss://external-api-ws.kalshi.com/trade-api/ws/v2`, with the same three authentication headers on the handshake and signature text `timestamp + GET + /trade-api/ws/v2`. A client must support custom handshake headers; the basic browser-style WebSocket API is insufficient. Consult current channel docs for subscriptions, initial snapshots, deltas, sequence/gap handling and resynchronization. Never claim a complete reconstructed book after a delivery gap.

The initial bounded probes made ten serial REST GETs (one-second completion spacing), including one 404 and two empty discovery results. Running the documented example added one successful Iowa book GET, for eleven requests total. No automatic retry, WebSocket, persistent collection or service activation occurred. Original local probe responses are in `%TEMP%/moloch-kalshi-read-probe-results.json` and `%TEMP%/moloch-kalshi-election-probe-results.json`; these temporary evidence files are not distributed or guaranteed durable.

## Official references

- [Key types and Ed25519 signing](https://docs.kalshi.com/getting_started/api_keys)
- [Read-only scopes](https://docs.kalshi.com/api-reference/api-keys/create-api-key)
- [Event discovery](https://docs.kalshi.com/api-reference/events/get-events)
- [Order books](https://docs.kalshi.com/api-reference/market/get-market-orderbook)
- [WebSocket setup](https://docs.kalshi.com/getting_started/quick_start_websockets)
