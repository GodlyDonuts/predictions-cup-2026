# Paper recovery over existing REST publications

`Start-PublicRecovery.ps1 -Name <new-name> -Initialize -KeepSession` starts an
independent prospective cohort reading the single collector's existing
`public-books.jsonl`. No platform request, credential read or contest order is
implemented by this consumer. Its source is verified normalized REST snapshots,
not a synchronized market stream or complete trade tape. REST expiry is unknown.
Original fast-stream cohorts and official-account accounting remain separate.

The frozen source scope contains up to 256 instruments, using existing structured
market/exchange identities. Coverage fields distinguish configured instruments
from observed histories and usable benchmarks. A configured instrument is not a
claim of a currently collected or executable quote. Each independent wallet still
has 100 SUSQ of counterfactual starting cash, 100 cumulative purchase budget, 25
per-market cost, three concurrent markets, state and unknown-identity limits,
10-share orders and at most 1% of displayed depth. Wallet capital is not additive.

Nine predeclared variants have 1-second and 3-second simulated latency wallets:
historical-dip control, recovery-confirmed native entry, or recovery-confirmed
one-tick-lower limit; target/stop pairs 0.01/0.03, 0.02/0.02 and 0.03/0.02. All use
zero added fees and adverse one-tick price padding constrained by the exact limit.
Those parameters are experiments, not profit forecasts or an optimal strategy.
Stop triggers can be followed by gaps or missing depth and do not cap losses.

The measured source cadence was roughly 39–63 seconds per tracked instrument,
with longer gaps, whereas the separate realtime recovery source rotated around
139–145 seconds. This cohort declares a 120-second order lifetime and ten-minute
recovery context, with a three-minute maximum observation gap. It does not
retroactively modify the earlier 30-second experiments. A blind gap starts new
context: an old low cannot manufacture a new rebound. Four independently
advanced, adequately spaced verified observations must show a two-tick decline
and at least two observations after the low, ending in a one-tick rebound with
the last bid not falling. Pre-cutoff verified observations can warm feature
context, but never create an order. Decisions and fills start after the cutoff.

Every source record is checked with `applyBookPublication`, including exact
tournament/scope, source revisions, request/receipt/publication clocks, invalidation
barriers, book depth and hash-chain continuity. The bank freezes the original
source prefix, publication session, universe, identities, policy, variants and
imported code hashes. Session rotation or source replacement fails closed; do not
adopt a new session silently. Collector STOP/closure invalidates available marks.
Consumer STOPs, singleton ownership and durable append guards are enforced.

Each consumer batch records its actual local availability. Older orders consume
the entire ordered evidence batch before expiry or new decisions; a delayed
consumer cannot erase an adverse in-window observation or backdate an entry.
The source-evidence watermark must reach the order deadline before an order is
classified expired. Only a later advanced source version with receipt and
confirmation inside the lifetime and after simulated latency can support a fill.
Depth is consumed once per order/version. These are observed counterfactual
executable opportunities, not proof of real queue priority or immediate fills.
Unobserved windows remain explicitly censored. An invalidation can end a pending
paper intent under the declared strategy; its hypothetical cancellation is not an
actual acknowledged exchange cancellation or proof that nothing could fill in a
blind interval. Interpret results with that limitation.

Accounting uses exact micro-SUSQ arithmetic. Partial-sale realized results persist;
round trips count only once inventory is completely flat. Losses and stop exits
remain in the ledger. Stale or thin bid liquidation marks remain unavailable.
The journal is fully replayed on restart against original publication bytes and
consumer times; changed banks, code, source prefixes or checkpoints are rejected.
Restricted runtime journals stay outside GitHub. Compare variants only on future
untouched outcomes, including unsuccessful signals, losses and censoring. A
selected winner, few trades or many correlated wallets cannot establish alpha.
