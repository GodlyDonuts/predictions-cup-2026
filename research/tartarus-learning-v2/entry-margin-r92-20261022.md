# R92 future-entry margin revision

The human targets at least one actual filled order per minute. Execution891/qid21634 reached the exchange at12:34UTC as a1,404-share NO bid at0.890, then expired unfilled. An old `native_reserved_not_dispatched` diagnostic was a staging observation, not final dispatch status. Original intent, attempt, ACK and closed records establish the real lifecycle.

Its declared expected payout was0.896445 per share. A0.005 entry margin rounded its acquisition ceiling down to0.890. With the same numerical forecast, a0.001 margin permits0.895: still below declared expected payout, but more aggressive. This arithmetic example does not establish that0.895 was an executable ask or that the uncalibrated forecast is correct.

## Future policy

The optional pinned execution policy now supports explicit six-decimal positive `entryMargins` for its named execution-mode scopes. Proposed value is0.001000 for the eleven added maker scopes:1010,1017,1057,850,1069,1011,983,1016,891,1012,1013. These scopes retain their three existing execution modes,1,250-principal review limit, quantity cap, original FIFO exit objective and0.005 exit profit floor. Other review entry margins remain unchanged. Account caps, funded bank, attribution, shared limiter, STOP and unknown-acceptance handling remain authoritative.

A future review records the exact owner-policy hash and margin in forecast evidence, with a genuinely new public quote observation. The exception allowance and contract identity evidence are retained. Price forecasts may change normally with new data. No original event, review, fill or cost is rewritten. Open targets are skipped and typed generation validation/current physical checks remain required. The revision is a strategy choice under existing execution-owner authority; it is not a calibrated confidence interval or guarantee of profits.

## Checks and rollout

Thirteen reload checks pass locally and on the VPS, including policy pin/scope/positive-decimal validation, skipping busy targets, executable-tick arithmetic and no repeated refresh after a successful revision. A real fresh public capture for891 passed the existing typed review inspector in a staged generation. This qualification creates no account client or order. The same existing updater will publish free-target revisions through the single worker after healthy recovery; no runtime replacement is required. Acceptance, fills and economic results require separate live evidence.
