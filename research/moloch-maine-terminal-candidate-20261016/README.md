# Maine Democratic Governor YES: inactive candidate

Prepared October 16, 2026, 19:04 UTC. This is a proposed V2 terminal-value review for market176/exchange865/contract177, with principal at most2,000. Root alone reviews activation and fresh physical facts. No account request, SSH, order, adoption, runtime or control change was made here. Parent reported this exchange flat at18:27; that is not a current account proof.

## Actual primary inputs

[Call the Map](https://www.callthemap.com/governors/maine) publishes **95.4%** for Hannah Pingree, explicitly modeling independent Rick Bennett alongside the Democrat and Republican. Its page distinguishes candidate win chances from vote shares and a two-party margin. Original bytes were received **19:01:57.654209 UTC**, with publisher `dateModified` **17:57:26 UTC**. The point estimate is not a calibrated Maine rare-tail lower bound.

The current [Maine Secretary of State election page](https://www1.maine.gov/sos/elections-voting/upcoming-elections) links the **FINAL.xlsx** candidate list. Exact `GOV` rows8–10 identify Rick Bennett Independent, Robert B. Charles Republican, and Hannah M. Pingree Democratic. This avoids confusing Hannah with the House candidate sharing her surname. The official [general-election candidate file](https://www1.maine.gov/sos/sites/maine.gov.sos/files/inline-files/2026%20General%20Candidate%20List%20-%20FINAL.xlsx) was received **19:03:00.555849 UTC**. Workbook modification is September9; its original publication time is unknown, explicitly represented by the documented `publishedAtMs:0` sentinel. Original file bytes, personal addresses and document-author metadata remain private.

Maine says its general gubernatorial election is decided by **plurality**, unlike federal ranked-choice contests. [Official voting explanation](https://www1.maine.gov/sos/elections-voting/ranked-choice-voting-frequently-asked-questions).

[Scrutinel](https://scrutinel.com/race/me-gov/) displays94% but expressly excludes Bennett from its two-party figures: that is not an unconditional corroborating probability. Its assertion that no public poll includes Bennett conflicts with the other publisher's candidate-field inputs; the discrepancy is retained, not resolved by averaging. [99chambers](https://99chambers.app/governor/me) displays98% and97% in separate sections; neither replaces the selected estimate. These models share political inputs, and geographic breadth is not independent risk. Kalshi's historical quote is predictive context, not fair value.

## Contract and economics

The original retained official SIG node, received **15:04:01.431 UTC**, explicitly identifies Democratic Party Winner, Governor of Maine, General, November3. Canonical root SHA is `5e768c0f5224b76620be03b0d08a9eb433c300d329491b1cd058ea47b42801e5`. Ordinary Pingree victory is mapped to Democratic YES as an explicit operator assumption; affiliation, replacements, resolver treatment and refunds remain unresolved exceptions. Bennett is included in the chosen ordinary forecast rather than silently assumed away.

Keep the existing stress assumptions: `p=.954`, `e=.03`, exceptional payout floor0, margin `.01`. Thus `(1-e)*p=.925380`; margin-adjusted value `.915380`, tick-floored ceiling **.915**. The 3% allowance is not an empirically estimated exception frequency or guaranteed bound.

The historical `.875 ×2351` ask is not current executable depth. It requires `p>(.875+.01)/.97=.912371134…` under these assumptions. A sensitivity p=.92 gives ceiling `.882`; p=.91 gives `.872` and fails that old ask. Maximum principal remains2,000, so the whole historical displayed quantity is not a funded commitment. No expected trade return or actual fill is claimed.

The draft preserves the existing run/bank and V2 actual-settlement objective. Nominal settlement is **November4 at17:00 UTC**; delay means hold for actual settlement, not a fabricated payout. All three source reference TTLs are24h. Earliest expiry is **October17 at15:04:01.431 UTC**; expired source blocks entry, not automatic liquidation. Current physical root, account, price/depth, capital and source eligibility still govern every dispatch.

## Reproduction and handoff

`review-input.inactive.json` is a typed draft with no activation status embedded into its schema; inactivity is recorded here and in `evidence.json`. The evidence file pins original byte hashes, receipt clocks, failed public fetches and sensitivity calculations. Old `posting.xlsx` links returned404/403; the successful file is the current official **FINAL.xlsx**, not a silently substituted page.

Run `python -B validate_candidate.py PRIVATE_ORIGINALS_DIRECTORY` from this folder. Original files are in `LocalAppData/Temp/moloch-maine-terminal-candidate-20261016`. The checker verifies exact raw hashes, GOV candidate/party rows, original node/root/timing, forecast text, clocks, review hash and Decimal arithmetic. It grants no runtime/model authority. Root must perform actual typed Rust/source validation, financial review and fresh physical reconciliation before activation.
