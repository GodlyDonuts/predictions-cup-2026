# Moloch actual contest runtime

This Rust executable joins the native strategy and ledger to the official SIG
API. It is the sole production account collector/writer; historical paper
wallets do not authorize another account client. For the deployed version,
active settings and actual results, use the [research baseline](../../docs/MOLOCH_RESEARCH_BASELINE.md).
Public trades and order books never credit actual fills.

Typed GET observations have at most three attempts for temporary connection,
body parsing, 429 and server failures. The shared queue enforces any existing
Retry-After delay between attempts. Observed HTTP status and Retry-After
survive even a non-JSON response. Schema/scope, ownership, durability and client
errors remain terminal. Placement and cancellation never use this read retry.

Run "moloch-live-runtime preflight /absolute/control.json" for authenticated
account observation without order submission. The "run" command requires the
explicitly funded live allocation, pinned activation/binary, exclusive OS
collector lock, and absence of run-local and shared EXECUTION_STOP markers.

The deployment control selects an absolute private credential file, one durable
account queue, tournament, exchange/market mappings, and immutable strategy policy.
The Unix provider checks ownership, mode and inode identity. It never provisions
credentials. Neither API keys nor ephemeral WebSocket JWTs belong in config,
fixtures, receipts, logs, or Git.

The runtime uses one serialized REST transport and the native Phoenix WebSocket
adapter. REST requests have a persistent 100-read rolling ceiling and 600 ms
completion pacing; contest orders retain 24 writes/minute and 2500 ms pacing.
Network latency lowers the observed request rate. A token mint consumes a read
admission, is restricted to at most one per minute, and uses the headroom below
the published 30-write limit if classified as a write by the server. Normal
refreshes occur every 30 minutes. Tokens remain in memory.
The Realtime origin comes only from the authenticated official token response.
Custom HTTPS origins and path prefixes are supported; credentials in URLs,
insecure schemes and private resolved network destinations are rejected.

Stream joins and refreshes invalidate coverage. Complete REST tape windows and
fresh books restore it. Revision gaps, compact batches and dropped connections
cannot silently authorize entries. A late gap extends the REST recovery window
back to its actual invalidation. The strategy's current-flow gate accepts only
verified engine events; REST reporting IDs are never relabeled as engine matches.

Every placement has an immutable logical quote, exact wire request, idempotency
key and permit recorded and synced before the physical attempt. The separate
hash-chained dispatch journal survives a crash before the financial ledger
commits the official acknowledgment. Unknown acceptance stops replacement
submissions and requires reconciliation; it is never treated as rejection.
Malformed, changed or torn journals fail closed.

Only owned official per-order fill receipts change the live wallet. Outcome
sign identifies YES/NO; the owned order identifies buy/sell. Actual execution
prices, admission resizing, partial fills, duplicate fill IDs, FIFO costs and
cancel/fill races are accounted separately. A closed order releases remaining
funding only after an independently later complete fill observation. A local
expiry, cancellation request or public trade cannot release an actual liability.

The historical zero-fee policy is checked against exact account cash before
new placements. Actual global cash must equal the initial account snapshot plus
the attributed wallet's cash change. Selected physical holdings must match
attributed inventory. Human-authorized inherited holdings are managed only after
the original receipt-bound adoption and exact opening-lot attribution. Unadopted
holdings and foreign resting orders remain separate. Owned cancellation
remains possible without a healthy market stream or matching cash.

Native inventoryAON is a planning label. The ordinary order endpoint has no
atomic-order guarantee: production submits a covered limit order and accounts
actual partial execution. It does not report simulated all-or-nothing fills.
The separate official multi-leg endpoint has its own all-or-none transport and
durable lifecycle; ordinary orders cannot borrow that guarantee.

The status.json file combines actual execution mode, stream health, scope
recovery, physical order counts and account queue state. The engine's own
networkRequests field describes its network-free calculations; use the
transport queue, private receipts and dispatch journal for network activity.
Correctness tests and active connectivity do not establish profitable trading.

Market-wide broadcast frames are also written to a separate private hash chain,
bounded to 128 MiB per process. It records no authentication messages and makes
no extra API requests. A recording error disables that diagnostic path without
changing order or inventory management. Subscription boundaries are explicit.
The moloch-breadth executable verifies an immutable journal prefix and ranks
fresh spreads and recent two-sided relative flow using original nanosecond
clocks, prior received books and stable price regimes. Its output is discovery
evidence, not executable profit, queue priority or authorization to trade a
new market. The existing all-market mandate still requires fresh official
reconciliation, native funding and physical authorization for each order.

The service runs an immutable versioned binary. Future work is built separately.
Do not hot-replace an executable, mutate the funded policy, reuse a paper bank,
reset an active bank, or extend the entry deadline through an ordinary restart.
After the entry window ends, a valid existing live bank can restart to manage
its attributed orders and inventory.
The human also authorized daily entry windows through the fixed competition
campaign end. Only the explicit durable daily-window transition implements that
mandate; an arbitrary restart does not renew it.

Reviewed forecasts can originate independently of ordinary maker proposals.
Omitted execution policy defaults to joining bid; explicit reviewed policies
can allow one-tick improvement or a bounded marketable limit with a resting
remainder. Only the matching opaque native/physical capability permits that
BUY. Current reviewed holdings, exits and cohort rearm are described in
[fundamental execution](../../docs/MOLOCH_FUNDAMENTAL_EXECUTION_20261016.md).
Optional execution diagnostics and the empirical maker gate are currently off;
their presence in source does not imply production activation.
