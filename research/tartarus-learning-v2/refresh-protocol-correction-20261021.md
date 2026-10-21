# Refresh protocol correction — October 21, 04:56 UTC

The earlier V2 experiment incorrectly treated a healthy token refresh as sufficient to retain validated book baselines. SIG explicitly requires REST refetch after token refresh even when subscriptions remain open. [Official API documentation](https://sig.thesuper.market/api/v1/docs).

The queued034 handoff was cancelled before it could stop033 or create a new run. Actual watcher2124 exited with cancelled_before_handoff at04:52:55UTC. The active033 learner and both completed CPU comparisons used the original legacy mode; production did not use V2. Earlier V2 source-study outputs remain immutable records of that experiment's behavior. They do not prove conformity with the documented refetch requirement.

V3 is the corrected default for new experiments. A healthy refresh retains channel identity and valid pending scoped receipts, invalidates book baselines and requires paid REST reads to validate them again. Genuine closure/expiry still rejoin. Old token timers remain fenced. V1 and V2 remain explicit reproduction profiles. Budgets, unknown-acceptance handling, reward and action choices are unchanged.

63 focused tests pass, including baseline refetch and once-only delayed passive-fill receipts. The actual-source V3 probe passed in20.846s: same paid requests and financial bytes, preserved channel, invalidated baseline. [Exact probe](refresh-v3-real-source-20261021.json). Replacement watcher21864 waits for all four completed members before its capacity check and handoff; the original V2 queue is cancelled. [Corrected plan](population-034-corrected-refetch-plan-20261021.json). Full raw-topic replay parity, source/calibration qualification and independently positive ROI remain unproven.
