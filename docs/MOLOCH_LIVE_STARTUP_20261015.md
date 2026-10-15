# Moloch production startup — October 15

Human instruction: start the existing strategy in production and develop further
improvements separately. This is actual trading in nonredeemable contest
SUSQies. It is not evidence of a profitable strategy.

## Verified observation

At 2026-10-15 07:05:05.992 UTC, the VPS user service moloch-live.service was
active, PID 85462, running the immutable rust-live-v3 executable. WebSocket
health was true; book and tape recovery were complete for all three selected
markets. The live ledger had zero actual orders, zero fills, zero owned cost,
zero reservations and zero realized P&L. One unique market trade had been
observed. Those are dated observations, not current account state.

The immediately preceding account preflight showed 70,850.43 SUSQies, 28
inherited positions and no open orders. Its newest historical account fill
remained October 9, 2026, 06:00:43.666 UTC. Other devices being stopped was
human-reported; these observations cannot prove remote process absence.

## Active boundaries

- Run: moloch-live-20261015-v1.
- Funded actual allocation and committed-cost ceiling: 40,000 SUSQies.
- Per-market cost ceiling: 10,000; maximum submitted quantity: 10,000.
- Eligible exchange/market pairs: 998/309, 1044/355, 846/157.
- Exchange 1048 was excluded because an inherited NO holding already existed.
- Entry cutoff: 2026-10-15 14:44:34.413 UTC. Ordinary restart does not renew it.
- One persistent account queue: maximum 100 read admissions/minute with 600 ms
  completion pacing, and 24 contest writes/minute with 2500 ms pacing.
- Actual observed REST throughput was about 65 requests/minute after warm-up;
  network latency prevents equating the configured ceiling to achieved speed.
- The old capture service remains inactive. The same kernel collector lock
  excludes another cooperating collector on this VPS.

Inherited positions and the old paper bank were preserved. The paper bank's
simulated cash and fills were not imported into the live allocation. The live
policy's current-flow gate still requires both relevant engine trade-flow
observations; an active connection alone does not require an order.

## Deployment and validation

Implementation: 9b960897 and custom official Realtime origin support:
2cc16d2054162a8f98652fb550587c90d8cbe8dd, on codex/moloch.
The live binary SHA-256 is
d6fcbbc5055088ce2918630230a419f2f0f289550cbc3d659c8e7e6e0263f9bf.

134 checks passed on Linux ARM64: 59 transport/queue/credential checks,
63 native checks, and 12 live feedback/authority/dispatch checks.
The release build also passed with the dependency lockfile.

An initial REST-only deployment could not satisfy the existing engine-flow
gate. Its upgrade preserved the financial engine identity, actual bank,
account rate history and entry deadline. A stale process lock was removed only
after proving its recorded process absent while holding the kernel collector
lock; the queue journal was preserved. A subsequent hostname restriction
stopped startup before any order attempt. The fixed adapter accepts a custom
HTTPS origin only from the authenticated official token response, rejects
private resolved addresses and keeps short-lived tokens out of persistence.

The Unix API key file and directory were provisioned with modes 600 and 700 and
verified by read-back without logging the key. Deployment controls, credentials,
raw private receipts and financial journals remain outside Git.

## Operations

Use the service manager and the current PID together with the run's status.json;
a status file alone can outlive a failed or replaced process.

    systemctl --user status moloch-live.service
    journalctl --user -u moloch-live.service --no-pager -n 30

The run directory on the VPS is
/home/archon/apps/moloch/shared/live-production-v1.
Its EXECUTION_STOP, or the shared EXECUTION_STOP, prevents new dispatch.
Unknown order acceptance requires source reconciliation and never triggers a
blind replacement. New strategy builds belong in a separate staging release;
the running binary and funded policy must remain immutable.

Primary venue contract: https://sig.thesuper.market/api/v1/docs.
