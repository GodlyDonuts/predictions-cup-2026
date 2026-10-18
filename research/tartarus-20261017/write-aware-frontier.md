# Whole-account sequential frontier — October 18

Production R44 restarted at09:42UTC after an exact unadmitted-account-expiry
recovery and is restoring its original financial history. This research changes
no production review, activation or order.
[Deployment evidence](checked-production-lane/r44-account-expiry-and-entry-fairness.json).

The new research search includes integer BUY quantities, whole FIFO SELL
tranches, at most 129 writes, 10,000 shares per write, the actual funded bank,
10,000 per-market cost and 12 **total non-inherited** markets. Existing positions
consume those 12 slots. All modelled sales precede purchases; acknowledged or
unexecuted sales never become actual acquisition cash.

Five declared views remain separate: three external-price/dependence
sensitivities and two retained operator/CTM views extended with conditional
price references. None is calibrated. The full matrix covers 139 contracts and
all 23 held sides; candidate projection preserves the complete holdings and
joint field/model distributions. The quotes and SIG books retain their original
clocks. Invalidated books are hypothetical future liquidity, not reactivated
current depth. Zero fees, zero terminal debt and retirement of proved untouched
planning drafts are explicit conditional assumptions requiring physical review.

The bounded MILP explores the trade-off between wealth in one specified world
and the weakest declared monetary expectation. It does not prove a global
optimum. Independent integer accounting checks shared depth across complement
actions, original FIFO, every refund-lot breakpoint, write count, prefix cash
and all market limits. The no-fill account is retained. Earlier candidates were rejected for miscounting existing fresh-market slots; a corrected search replaced them. Reconstruction from original adoption and actual sells leaves 14 exchanges with original inherited lots and nine current non-inherited markets. Historical adopted IDs alone do not establish remaining inherited lots. Numerical incumbents failing the exact audit are rejected. [Exact results](write-aware-frontier-20261018.json).

| Conditional candidate | Writes | Native projection | Weakest declared expected wealth change |
|---|---:|---:|---:|
| Maximise weakest declared money, within this candidate family | 25 | 29 contracts / 70 states | +3,759.060483 |
| Require 250,000 wealth in the selected world | 49 | 33 contracts / 76 states | +233.372490 |
| Require 500,000 wealth in that world | 79 | 33 contracts / 75 states | −9,491.112453 |
| Require 925,300 wealth in that world | 125 | 34 contracts / 82 states | -38,121.932000 |
| Candidate reaching the dated first-place anchor | 129 | 35 contracts / 102 states | -55,831.206017 |

The positive declared-mean candidates have **zero** first-place mass against the
dated 1,081,241.68 flat anchor in every sampled view. The final rank-risk candidate
has flat-anchor strict mass between 0 and 18,981 ppm across the five views, with
zero mass in the adverse fields. These are model sensitivities, not estimated
real-world odds. Its minimum prefix wealth is 94.03 and maximum loss versus
the modelled hold account is 109,295.755; it is explicitly negative monetary
expectation. A simulated target hit is not positive alpha or a trading result.

[Exact sequence auditor](../../moloch/tartarus/sequence_audit.py) has six focused
checks, including shared-depth double use, unfunded purchases, refund interior
losses and existing non-inherited slots. The separate scenario-oracle capacity
module plus original rotation bound passes eleven checks. Its
[dated sensitivity](joint-scenario-capacity-20261018.json) retains exceptional
mass as unbounded rather than assigning it zero probability or zero capacity.

[Prototype](write-aware-frontier-prototype.py) takes an explicit private input
directory and output path; account originals are not committed. It requires
NumPy/SciPy and makes no network requests. The next production step is a
fresh-source, current-account owner review through the existing worker. No
candidate in this report has been installed or claimed profitable.
