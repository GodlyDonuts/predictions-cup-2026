# Bid-anchor ask-concession paper strategy

This is a distinct strategy hypothesis over the existing verified public REST capture. It asks whether an executable ask concession below a stable prior bid anchor predicts subsequent bid recovery after spread, depth and delayed fills. It does not assume that any dip will recover or that historical bids are fair probabilities.

The fixed signal uses the first unique quote in each minute over ten minutes, at least five observed minutes spanning four minutes, and no observation gap above ninety seconds. Only prior observations enter the reference. Prior bids must lie within a 0.015 range. The padded ask must be at least 0.010 below the prior median bid less an exit tick; spread is capped at 0.020, bid collapse at 0.040, and the ask must support one share at one-percent participation. This is one fixed definition, not a parameter sweep selected for historical profit.

Two separate counterfactual wallets use one- and three-second request latency. The shared exact FIFO engine requires a later request, independent source version, original limit, one adverse tick and one-percent depth per level. Each wallet has 100 starting SUSQ, one-share orders, ten aggregate exposure, ninety cash reserve, three concurrent markets and a nonresetting 100 cumulative purchase budget. Capital in these comparison wallets is not additive. Explicit fees are zero.

An unfilled buy preserves its original bid anchor for its bounded lifetime; its own newly observed trough cannot retroactively move that reference. Current cost/concession, spread and collapse gates still apply. Exits use a 0.010 modeled take-profit, 0.020 stop and five-minute hold, followed by a fifteen-minute closed-position cooldown. A stop does not cap losses. If the best bid lacks capacity, a risk-exit replacement may lower its limit to the worse depth-eligible bid and reset its decision clock; it cannot fill at the newly chosen price on the same observation. Missing liquidity remains unsold inventory.

The cohort is evaluated in foreground from the shared capture. It creates no background collector, account client, key, scheduler or competition order:

```
node live-paper/bid-anchor-replay.mjs --name bid-anchor-20261003-v1
```

The existing cohort was frozen at **2026-10-03T01:58:34.485Z**, bank hash **2365a1ea4b1c1ee48208f0fbe4b6be39f0320fd163245f4fd6a3699b70786ed5**. Its immutable definition and separate report live under the restricted runtime `bid-anchor-cohorts/bid-anchor-20261003-v1`. The original source-prefix bytes/hash/sequence and code identities are checked before replay. `--initialize` requires a new name and refuses overwrite. Never retrain or edit an active definition.

Replay verifies the full publication chain, preserves source invalidations and sessions, and allows entries only on post-cutoff requests. It assumes prompt observation at publication time; one- and three-second delays are sensitivity cases, not proof of actual consumer latency or actual fills. Synthetic correctness tests do not prove profitability. Prospective results, missing outcomes, open inventory and quote coverage must be assessed before any competition activation. The original executable-edge bank and all retired experiment data remain untouched.
