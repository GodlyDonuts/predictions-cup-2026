# Canonical NO package observation

The existing collector now supports a separate, prospective conditional package
capacity monitor. It creates no API client, background process, order or paper
fill. Every fourth collector cycle can replace two optional depth reads with
adjacent reads of the reviewed pair. Official/paper risk reads and their order
are retained; unknown read priorities, other paired reservations and exclusions
are protected. Four book slots and the serialized 20-read/minute allocation are
unchanged. If both legs do not fit, the original plan is retained.

The canonical API book is YES-normalized. The official trading guide states that
uncovered SELL YES is BUY NO at the complementary price. Therefore this monitor
uses `1 - YES bid`, with the same exchange, quantity and source version, for NO
acquisition cost. It does **not** flip YES asks or invent an independently
observed native NO book. Results retain `nativeNoObserved: false` and the explicit
representation `documented_same_exchange_yes_bid_complement`.

The original `payout-capacity.mjs` native-outcome evaluator is unchanged. The new
`canonical-no-capacity.mjs` accepts the different, documented representation.
Its joint ladder walk uses integer micro-SUSQies. The frozen observation policy
uses 1% of each price rung, one adverse 0.005 tick per leg, zero fees under the
human instruction, a 0.01 minimum ordinary marginal edge, maximum package cost
25 and maximum isolated-leg acquisition cost 5. Those are observation limits,
not permission to trade. Full displayed depth is a separate sensitivity, never
the admitted quantity or a fill guarantee.

Both request starts must follow review availability and the prospective cutoff.
The entire acquisition window must fit 10 seconds; request age and account age
are limited to 20 seconds. Complete existing account-cache eligibility must be
known before both requests. Own positions or open orders exclude either leg.
Missing, future, stale, skewed and malformed evidence cannot produce capacity.

The initial NH review uses Daenerys's exact Info/ballot transcript evidence.
Rules hashes bind that manual attestation; they are not live API metadata proof.
Ordinary D/R/other winner scenarios are shown alongside a zero-payout fusion or
administrative stress. Same-ballot dual-party candidates in other offices are
contrary evidence to any claimed anti-fusion guarantee. `stressedMinimumProfit`
can be the negative full acquisition cost even when ordinary modeled capacity
is positive. Neither scenario completeness nor profitable resale is asserted.

The collector's already verified public-book journal supplies provenance. The
new named bank freezes code, review, policy and source-prefix identities without
changing existing experiment banks or wallet journals. Each observed pair must
advance both versions and start both requests after the prior pair's receipt.
Results append to a hash-linked, fsynced journal, with original minimal account
eligibility and actual evaluation time. Repeated frames cannot be reinterpreted
as additional opportunities. Restarts restore the original journal; code/bank,
source prefix and journal changes fail closed for this monitor. The collector
continues serving existing paper risk if this optional component fails.

Current results are exposed in `/api/stats` under `runtime.canonicalCapacity`.
The component is initialized only by a named restricted-runtime manifest.
Expired reviews stop scheduling; refreshed evidence needs a new named cohort.

Checks: `node --test live-paper/canonical-capacity-monitor.test.mjs` covers 14
unique correctness checks including an independent capital-boundary reference,
real publisher-to-monitor persistence, resume, mutation detection and retained
risk/read budgets. Synthetic checks establish behavior, not actual alpha.

Sources:
- https://sig.thesuper.market/docs/markets-and-trading
- https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5973997375
- https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5974024230
- https://www.londonderrynh.gov/DocumentCenter/View/3063

The collector additionally wraps the frozen monitor with canonical-capacity-guard.mjs. A separate fsynced high-water projection binds the accepted journal prefix, byte count, sequence and tail across restart, detecting valid-prefix truncation of the original journal while that anchor survives. Existing published stats also supply a bootstrap sequence floor. This does not authenticate deliberate coordinated rollback of both the journal and every independent anchor. Optional NH pairing is skipped if either leg is already a protected risk read, preserving held-risk order and avoiding nonadjacent measurements. The frozen evaluator, bank, journal and original monitor source remain unchanged.
