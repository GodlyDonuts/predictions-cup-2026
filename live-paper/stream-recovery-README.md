# Prospective recovery entry experiment

This independently frozen paper consumer shares the existing verified stream v2
gateway. It makes no API calls or competition orders, and keeps the original dip
wallets and official execution ledger unchanged. Initialize with
`./Start-StreamRecovery.ps1 -Name recovery-20261002 -Initialize -KeepSession`.
Its separate singleton lock prevents two recovery writers; it may run alongside
the dip consumer. Restricted runtime banks, journals and stats live under
`stream-recovery-cohorts/<name>`. The collector remains the sole network client.

Nine predeclared variants each have independent 1-second and 3-second latency
wallets (18 total). Entry modes are the unchanged historical-dip control,
confirmed recovery at the ask, and confirmed recovery with a limit one tick below
the ask. Exit target/stop pairs are 0.01/0.03, 0.02/0.02 and 0.03/0.02 SUSQ per
share. Their idealized break-even win rates are 75%, 50% and 40%; actual spread,
gaps, incomplete fills and holding exits change those economics. These are
experimental alternatives, not evidence of profitability or optimized choices.

Recovery needs at least four distinct verified bid observations over three
minutes, at least one second apart and no gap above 90 seconds. A two-tick drop
must precede the trough, then two subsequent observations must show at least a
one-tick rebound with the final bid not falling. Duplicate versions and later
consumer availability cannot manufacture confirmation. Previously verified
historical points survive an intervening data invalidation, but executable
readiness and pending orders do not. Points are discrete observations, not a
claim of a complete trade tape. Recovery points start after this cohort's cutoff;
pre-cutoff books only warm the minute-balanced historical benchmark.

Every variant retains price, spread, history, cash, concentration, depth and
cumulative purchase gates. Confirmed entries also require room to their stop
after the padded immediate liquidation spread. Zero added fees and one-tick
adverse slippage apply, capped by the exact fixed limit. A fill requires a later
advanced source version, actual recorded receipt/confirmation inside the order
window, the wallet's latency and verified continuity. Each source depth may be
used once per order. These are counterfactual executable opportunities, not
demonstrated queue priority or official fills.

Consumers persist their actual decision availability, raw source/checkpoint
hashes and exact micro-SUSQ accounting. Source-evidence watermarks prevent a
delayed consumer from discarding adverse in-window fills. All losses remain in
accounting; stale or thin liquidation marks remain null. Closed-round-trip win
rate and profit factor include complete trades only, with partial-sale realized
P&L retained separately in the wallet total. An invalidated/expired unfilled order
is not a zero-profit completed trade. Restarts replay the complete wallet journal
against original raw capture and reject changed code, banks or evidence. Never
overwrite an active bank to retrain or change rules; use a new named cohort.

Synthetic tests establish causal, persistence and accounting correctness only.
Judge variants on subsequent untouched captures, retaining all outcomes and
uncertainty. Many wallets on shared markets are correlated experiments, not 18
independent observations. Do not scale official execution from a selected paper
winner or a tiny sample.
