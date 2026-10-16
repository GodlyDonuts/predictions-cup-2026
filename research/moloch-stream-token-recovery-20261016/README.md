# Bounded realtime-token HTTP recovery

The October16 13:47:15 UTC production stop was an original SDK
`CONNECTION_UNCONFIRMED` from `Transport::realtime_token()`, according to the
execution owner's retained incident evidence. It was not a native DNS/TCP
connection failure. The execution owner restarted the same binary separately;
this source task makes no production changes or live recovery claim.

`stream_token_retry::TokenFailure` is a separate opaque, non-deserializable type
constructed only at that exact SDK method's error boundary. It accepts a narrow
set of original `TransportError` fields:

| SDK code | Required status |
| --- | --- |
| `CONNECTION_UNCONFIRMED` | absent |
| `BODY_UNCONFIRMED` | 2xx, 408, 429, 500, 502, 503 or 504 |
| `OFFICIAL_REQUEST_REJECTED` | 408, 429, 500, 502, 503 or 504 |

Every accepted case also requires `may_have_executed=false`, no order ID and no
API error code. Other errors retain their original concrete SDK type and remain
fatal: authentication/credentials, 401/403, malformed/oversized JSON, invalid
token fields or expiry, lease/STOP loss, unknown acceptance, queue/durability
failure. Native `ConnectionFailure` retains its original semantics. Text from an
arbitrary boxed error cannot select either typed retry lane.

The actual SDK source classifies `/realtime/token` as `write=false` and admits it
through the existing serialized account read queue. It durably completes the
request before returning its HTTP error; completion failure instead returns
`COMPLETION_DURABILITY_UNCERTAIN`, which is not retryable. A retry only requests
new authentication material, never an order or a replayed financial operation.

Both retry lanes share the unchanged `stream-recovery-budget.json`: at most
three admitted setup attempts in an inclusive rolling ten minutes. The worker
persists one budget charge before each mint/connect operation, including initial
startup, and never refunds or resets it on errors, success or restart. Exhaustion
stops the current recovery operation rather than waiting out that window.

The original durable mint marker imposes at least 60 seconds between mints.
`retry_after_ms` is the SDK's remaining persisted next-admission cooldown; the
worker also carries that delay into its cooperative wait. The SDK continues
enforcing the original persisted cooldown after process restart. No new limiter,
timer reset or account client is introduced. During waiting the existing worker
checks STOP/control and clock validity at most 200 ms apart; before every attempt
it rechecks resolved dispatch attempts and explicit global queue quietness
(unknown empty, active null, manual review false). Marker, budget or diagnostic
persistence failures remain fatal.

Auth validation, actual native connect, JOIN confirmation and durable subscribed
source events remain the original path. No failure branch marks a socket joined,
changes source clocks, declares tape coverage healthy or releases order funding.
The diagnostic records only the allowlisted code/status, attempt times and next
deadline; no token, URL, auth header or response body is logged.

Six authored regression tests cover concrete typed transient/fatal cases,
uncertainty and identity denial, mixed token/native failures sharing a reloaded
three-attempt budget, longer Retry-After without another charge, and malformed
clock/marker/uncharged/overflow rejection. These are source-only tests pending
the execution owner's ARM64 run; they do not simulate a live provider outage or
claim full worker HTTP/JOIN integration. Existing worker tests cover explicit
global queue quietness, and existing native-connect tests remain unchanged.
