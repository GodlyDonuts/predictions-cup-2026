# One bounded DNS GET recovery

Source 37 stopped at 11:49:28Z on a typed `System.Net.Http.HttpRequestException`, `NameResolutionError`, status zero. Its websocket was SUBSCRIBED, had 149 trades and no gaps, and was closed only because the REST script exited. The exact failed exchange-920 tape window subsequently returned HTTP 200. This is a concrete DNS-read failure, not an uncertain order submission or an authentication failure.

Optional `--dns-read-recovery` permits one additional GET admission per capture, only for a typed DNS failure with no HTTP response. The exact endpoint, cursor/window, method and existing headers are reused. Both attempts consume the existing persistent budget; backoff is at least sixty seconds and includes Retry-After delta or date. POST/token creation, HTTP response failures, timeouts, unknown failures and any second recovery are excluded. Default behavior remains stop-on-failure.

During that backoff the current websocket stays alive without another client or token. The failed financial scope is invalidated once; other scopes remain observable. A successful HTTP 200 JSON response alone clears the local read failure flag. Paper quoting in the failed scope still requires the ordinary later book/tape resynchronization. A failed second attempt remains terminal. Immutable sanitized read-failure events and a derived recovery-status file retain the evidence; no messages, headers or credentials are logged.

The retry rationale is the idempotent request guidance in [RFC 9110 §9.2.2](https://www.rfc-editor.org/rfc/rfc9110.html#section-9.2.2). The classifier uses the documented [.NET HttpRequestError enumeration](https://learn.microsoft.com/en-us/dotnet/api/system.net.http.httprequesterror?view=net-9.0). This is not permission to retry writes or blindly restart a collector.

Validation: 115 Node tests and ten PowerShell failure/secret-omission/recovery-guard checks pass. The exact failed live GET was probed successfully after backoff. A real automatic DNS recovery has not yet been observed; its runtime result must be checked rather than inferred from these guards.
