# Matched prospective entry comparison

The historical dip and shock-rebound signals now have a separate common-feed comparison. Existing frozen wallets and the official executor remain independent. This worker reads the sole collector's saved source, adds no API requests and cannot submit orders.

The fixed scope is the existing four-market panel: 377, 354, 277 and 305. It is not selected from the two successful or unsuccessful official trade outcomes. A newly named bank freezes the original source-prefix hash, future cutoff, strategy inputs, dependency hashes and shared execution rules. Prefix observations may seed references; they cannot create trades. Reusing or overwriting a cohort is rejected.

Both wallets use zero fees, one adverse price tick on each executed side, 1% of displayed depth, at most ten shares per entry, 25 SUSQ per market, 100 SUSQ of cumulative purchases, three positions, a one-second minimum delay before a subsequent independent quote, a 60-second entry lifetime, a 0.01 take-profit, a 0.03 stop and a 60-minute holding horizon. Sizing, capacity and capital are separate for each hypothetical wallet and cannot be added together. Native signal gates remain different. The dip compares the genuine historical center with the padded ask once; the rebound uses its native historical bid reference and recovery-room rule.

The older paper dip applied an additional edge cushion and excluded markets held by the real account. Its zero fills cannot be compared directly with official pilot results. This comparison retains exact valid public books even when followed by a matching account-exclusion diagnostic. It labels observed account exposure/open orders and missing or stale account context. Aggregate depth can contain our orders, so these simulations do not establish uncontaminated executable liquidity. Other failures, collector boundaries, stale data and missing depth remain visible. Unfilled intents and unfinished positions are never turned into wins or zero-loss completed trades.

Run from the workspace:

```powershell
.\Start-EntryComparison.ps1 -Name entry-common-20261002 -Initialize -KeepSession
```

On restart omit `-Initialize`. The worker verifies the full source and deterministically reproduces each recorded accounting checkpoint, then tails appended source bytes. A changed bank, dependency, source prefix or accounting state fails closed. Local append-only files are the trust boundary: head and boundary checks detect ordinary replacement and rewriting, but a full restart verification is needed to detect an arbitrary old middle-prefix modification during a concurrent append. An incomplete source tail remains `awaiting_capture`; no decision is made from it.

Files live under `LOCALAPPDATA/SigTradingCup/paper-runtime/entry-comparisons/<name>/`. Workspace/runtime `STOP`, runtime `ENTRY_COMPARISON_STOP`, or that cohort's `STOP` ends the worker. Inspect STOP provenance and a live lock before recovery. An optional local `entry-comparison-active.json` manifest selects the matching bank/name for the dashboard; it cannot enable trading.

The current capture sparsely samples most of the wider market universe. The fixed panel offers a better common denominator, but missing follow-up prices still censor outcomes. Expiry without a subsequent quote is different from an unprofitable completed trade. New results must remain prospective and retain all outcomes; neither a tiny number of fills nor model count demonstrates an edge.
