# Turnover worker: first complete live cycle

At **2026-10-16 23:11:28 UTC**, sole worker **565108 / source20543435 /
binaryd9f5cbc6 / nativeb5** is fresh trading, realtime healthy, has completed
37/37 selected scopes and has a matching-PID full-cycle status. The actual
executable hash matches; no execution STOP is present.

Since this release started: **8 official acknowledgments, zero new fill rows**.
The original bank has 241 cumulative fills, cash **70,762.850000**, owned cost
**24,680.835000**, new-trading realized **−181.360000**, inherited gross
**+45.130000**, combined realized **−136.230000**. ACKs are not fills, and these
attributed realized figures differ from dashboard daily valuation P&L.

The existing 95,579.915000 effective capital mandate remains in force. No ledger
reset, contribution recredit, source-clock renewal or extra account client was
used. These are dated observations, not a profitability claim.

## Original evidence

`manifest.json` pins six exact files. Health/report checks derive from the
existing worker and its original sealed ledger. The full private account ledger
and status are not published. Snapshot files retain their individual observation
times; this is not an atomic account snapshot or a future execution permit.

`v8-rollout.json` and `v8-operator.py` record the separate read-only observer
569117, bound to the actual565108 journal at22:54:58UTC. It uses the same pinned
producer/mapping, a new finite one-hour cohort, a disk floor and `Restart=no`.
The original incomplete V7 seal is unchanged. V8 status is observing at23:11:55;
its samples do not qualify a profitable model.

`diagnostic-summary.json` records another live-entry investigation: the
fundamental entry diagnostic is still22:30, while expired-exit refresh attempts
continue. The installed Oregon review changed aggregate limits without changing
its economic forecast; source `exit_refresh_needed` incorrectly treats any
review-hash revision as perpetual urgency. The source path is confirmed, but
the exact selected hint is not instrumented. A narrow semantic repair and a
separate current-lot eligibility repair are being checked; neither is deployed
by this evidence publication.

Capture and export were file-only and made zero SIG API requests. Raw bytes and
hashes were verified again after authenticated SCP and before Git publication.
