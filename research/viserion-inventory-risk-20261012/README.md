# Inventory-aware harvest experiment

V7 adds an explicit, separately bank-hashed inventory-risk policy to the existing funded V6 paper wallet. `inventoryRisk.maxHoldingMs` declares the holding deadline, and `inventoryRisk.maximumAdverseTicks` declares the adverse-price threshold. A position crossing either threshold retires that market from future entries in the cohort. Additional buy quotes are not created while inventory remains; an original pending entry may still fill partially within its original limits.

Risk-triggered sales may realize losses. They reserve only owned shares and still need independently later, latency-qualified executable depth. Existing supported partial fills are processed **before** remaining hypothetical buy quotes are retired. Risk stops cannot fabricate a fill, guarantee liquidation, or stop a live account process. The original V6, frozen banks, consumer and runtime are unchanged.

The adverse signal uses a stressed top price versus average cost; it is a risk heuristic, not proof that the entire position can be sold there. Later fills remain constrained to one percent of each observed level, with version-specific depth reuse protection, exact cash/FIFO accounting, original source/rule clocks and zero-fee scenario assumptions. For insufficient depth, the remaining inventory and unavailable liquidation mark stay visible.

## Predetermined synthetic comparison

The comparison declares a 30-second deadline and four-tick adverse move for short test paths. These values are mechanism-test parameters, **not optimal live settings**. Each path starts with the same 1,000-SUSQie wallet, 50 principal cap and original quote/fill clocks.

| Synthetic path | Original V6 final funded P&L | V7 final funded P&L | Meaning |
|---|---:|---:|---|
| Continued decline | -11.365000 marked, inventory retained | -3.480000 realized, flat | Risk exit reduces this constructed loss; original also adds inventory |
| Quick profitable rebound | +1.160000 realized | +1.160000 realized | V7 harvests at the earlier supported receipt |
| Stop then recovery | +2.420000 marked, inventory retained | -3.480000 realized, flat | Explicit counterexample: stopping can sacrifice recovery |
| Unchanged losing liquidation | -0.870000 marked | -0.870000 realized, flat | Recognizes the loss and releases capital instead of keeping it open |
| Liquidity disappears | Incomplete mark; 60 shares retained | Incomplete mark; 58 shares retained | Neither policy may invent a full exit |

The synthetic marks are counterfactual executable-depth scenarios, not competition valuation marks or actual profit. There is no aggregation or score-based parameter selection. The recovery counterexample is retained. These results establish risk mechanics and tradeoffs, not market edge or superiority.

Run from repository root:

```powershell
node --test paper/shared-reference-wallet-v7.test.mjs paper/shared-reference-wallet-v6.test.mjs
node research/viserion-inventory-risk-20261012/compare.mjs
```

Eleven checks cover later-fill timing, losing exits, partial buy/cancel ordering, thin exit depth, unavailable liquidity, ordinary profitable exits, frozen policy identity, NO-side complements and duplicate receipts, alongside the original V6 checks. `results.json` contains all paths, exact funded traces, separate bank hashes and source identities.

Next deployment step is a separately prospective bank and causal original-journal comparison through the existing owner's worker. No new collector, live orders, credentials or production activation are part of this experiment. Observe fill-conditioned loss, realized returns, remaining inventory and capital-hours under both policies before selecting or scaling a policy.
