# Source-backed settlement policy

`backend-rust` provides a finite `settlement-policy` command. It assesses supplied
research and sampled books; it does not send orders, produce fills, settle a
wallet, or replace the active probability cohort.

The intended objective is value after resolution. The existing probability
wallet has a two-day exit horizon. This separate policy holds while a current
forecast supports the position and proposes a reduction when the sampled bid,
after its adverse tick, reaches the upper supplied probability scenario. It can
propose that reduction at a loss: acquisition basis is not future fair value.
It has no arbitrary two-day timeout or price-only stop. An expired, conflicting
or unbound forecast requires review; its original source/assessment timestamps
remain unchanged. Held basis and independently available liquidation depth stay
visible. The supplied horizon never creates a settlement payout.

New exposure uses the lower supplied scenario, quarter Kelly at most, and exact
micro-unit arithmetic. The binary-payoff sizing fraction is
`(lowerProbability - purchasePrice) / (1 - purchasePrice)` times the chosen
fractional multiplier. It is a sizing heuristic. Subjective lower endpoints are
not calibrated confidence bounds, and greedy candidate selection is not joint
portfolio optimization.

Admissions include cash reserve, full acquisition cost, existing reservations,
cumulative purchases, per-order cost, portfolio exposure and all declared risk
groups. Shared race and national exposure must be explicitly grouped. Existing
cap breaches are reported without erasing inventory. Depth is limited to one
percent by the adapter. Multi-level acquisition reserves quantity times the
worst proposed hard limit, rather than just the cheaper weighted fill cost.
Liquidation requires sufficient depth for the full position; otherwise its
total value remains unavailable.

Book availability must precede the decision. Research must precede the original
request. Complete causal account observations must precede that request and be
fresh at consumption. Account positions prevent new counterfactual exposure;
open orders prevent all use of our own potential liquidity. Canonical NO
complements are diagnostic only and do not authenticate a REST submission price
basis. Every output retains zero fees and `orderEligible=false`.

Run a saved input with its SHA-256 and a new output path:

```powershell
backend-rust/target/release/baal-backend.exe settlement-policy --input input.json --expected-sha <sha256> --out output.json
node data/check-settlement-value-20261004.mjs
```

The second command reads the running probability report, verifies its exact
public-book prefix through the native publisher, and writes a separate immutable
diagnostic. It refuses truncated fill history or pending intents rather than
guessing cumulative purchases. It does not change the bank, wallet, active
binding, collector or competition writer. Raw account financial data never
enters its input or evidence record.

Initial real-state evidence is in `data/settlement-value-snapshot-1791144422978.json`.
The old WV assessment is expired; TN lacks a current book. No new acquisition is
proposed. This is a correct rejection and held-risk view, not a profit result.
The existing source wallet had +0.020 realized and +0.010 estimated total
liquidation P&L in that snapshot, with only three fills and one open position.
Those tiny counterfactual results do not establish a successful strategy.

`data/kansas-source-lead-20261004.json` records a new exact-market research lead,
with indicative source clocks and primary references. A published forecast point
can suggest a pricing discrepancy without establishing a conservative interval,
current contract interpretation, executable depth or profitable fill. The lead
does not create a bank or refresh the existing research.

Before a new prospective paper cohort: resolve the individual contract and
forecast evidence, freeze its probability method, source hashes and future
cutoff, and connect this policy to independent later-request fill/reconciliation
machinery with separate accounting. Existing experiments remain frozen.
Competition activation still needs the named return RELEASE/ACK and fresh
account reconciliation.
