# Oregon Republican NO: inactive terminal-value candidate

Prepared 2026-10-16 18:40 UTC for execution-owner review. `review-input.inactive.json` is a typed V2 proposal, not activation. Principal is capped at 2,000, with no orders, account requests, control changes or existing-position adoption by this researcher. Parent reported exchange 977 flat; this packet does not independently refresh account state.

The retained original SIG node identifies market 288, exchange 977, contract 289: Republican Party Winner, Oregon federal Senate, General election November 3. Its canonical root SHA is `ebe719c4fc2796abfc66e2669db45e84abf9c1c7310e00dc298f05f5c2c60d15`. Nominal settlement is November 4, 17:00 UTC; the proposal holds for actual settlement, including delay. Raw market creator metadata is excluded.

## Primary evidence and limits

Call the Map publishes Merkley win probability **99.5%**, distinct from projected vote share. The retained page was received at 18:34:55.251800 UTC; its publisher `dateModified` is 17:57:26 UTC. This supplies a model point assumption, not an Oregon-specific calibrated lower confidence bound. [Primary forecast](https://www.callthemap.com/senate/oregon).

The publisher describes joint Student-t election simulations and nested leave-one-cycle-out validation over 2018–2024. Its 786 Senate race-horizon rows are not 786 independent elections; reported Senate Brier score is .0543, MAE 5.869 points, with +2.746-point Democratic center bias. These aggregate checks do not establish rare-tail or contract-exception calibration. [Primary methodology](https://www.callthemap.com/methodology).

The official county candidate list gives Smith Republican, Merkley Democrat/Working Families, Dye Libertarian and Henry Progressive/Pacific Green. Ordinary Democratic victory therefore supports Republican NO only under an explicit party-resolution assumption. Working Families fusion, affiliation changes, replacements, independent outcomes, refunds and discretionary resolver treatment remain unresolved. The page has no known publication timestamp: `publishedAtMs: 0` explicitly records unknown, while its original receipt clock remains intact. [Official election listing](https://www.co.umatilla.or.us/departments/elections/general-election/contest-and-candidate-listing-for-the-november-3-2026-general-election).

Two retained cross-checks are not independent likelihoods and are not averaged: [Scrutinel](https://scrutinel.com/race/or-sen/) displays 99.7%, updated October 16 at 11:50 AM EDT; [99chambers](https://99chambers.app/senate/or) displays 98% prominently and 97% in another history section. Both values and their discrepancy are retained. Kalshi is predictive context only, never this packet's fair value.

## Economics and review boundary

With owner-proposed `p=.995`, exceptional allowance `e=.03`, exceptional payout floor zero, and margin `.01`:

`value = (1-e)*p + e*0 = .965150`; margin-adjusted value `.955150`; tick-floored entry ceiling **.955**.

At the historical `.925` ask, the probability threshold including margin is `.935/.97 = .9639175258…`. Sensitivity ceilings are `.940` at p=.98 and `.930` at p=.97. The 3% allowance is an operator stress assumption, not an estimated frequency or guaranteed bound. Maximum loss remains the allocated principal, and election/model risks are correlated. No current quote, depth or fill is claimed; the old `.925 × 1851` observation needs original fresh physical verification.

The original node receipt is October 15 12:37:03.505 UTC. Parent explicitly approved a **48-hour immutable reference TTL**, without changing that clock or the mandatory fresh physical contract-root check. Forecast/ballot TTLs are 24 hours. The earliest source expiry limits new entries to **October 17 12:37:03.505 UTC**; expiry does not liquidate a settlement holding. Existing NC and immutable bank/run bindings are preserved.

`evidence.json` contains exact original-byte hashes and capture clocks. Originals remain private at `LocalAppData/Temp/moloch-oregon-terminal-candidate-20261016`; the official node copy is `official-contract-node.json`. No raw HTML or creator metadata is committed. Run `validate_candidate.py PRIVATE_ORIGINALS_DIRECTORY` for bounded byte/source/decimal checks. Root must still run the actual runtime typed review/source validation and review the unknown-publication sentinel before any activation.
