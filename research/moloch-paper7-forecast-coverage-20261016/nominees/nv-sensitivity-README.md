# Nevada: finite inactive stress nomination

For independent root review only: **market 183 / exchange 872 / contract 184 / Republican Governor of Nevada YES**. The original CTM candidate-win model point is **Joe Lombardo .783**. The page's original `dateModified` is Oct 16 at 17:57:26 UTC; receipt is 22:51:15.121401 UTC, raw SHA256 `a32349a41a5e3db86bb8c4fc7c8e04fb8aa99b9d1264f835429ffb1f938a5980`. A page modification clock does not independently verify the forecast's first publication or availability time. No new forecast, source observation time, reviewer, approval, allocation, Runtime ReviewInput or control generation is minted. See [nv-sensitivity.json](nv-sensitivity.json) for all original pins and exact decimal results.

The illustrative mixture is `V=(1−e)×.783+e×0`; entry ceiling is `floor_.005(V−.01)`. **For this arithmetic only**, .783 is treated as the ordinary-regime Republican-win probability. The publisher does not identify that conditional probability or calibrate the imposed exception mass. The four e values below are unestimated stress choices, not statistical bounds, a proven dependence model or qualifications of the full field.

| Assumed exception mass e | Assumed payout/share V | V less .01 margin | Tick-floored research ceiling | Ceiling less old .460 ask |
|---:|---:|---:|---:|---:|
| 3% | .75951 | .74951 | .745 | .285 |
| 10% | .70470 | .69470 | .690 | .230 |
| 20% | .62640 | .61640 | .615 | .155 |
| 30% | .54810 | .53810 | .535 | .075 |

All four pass **historical price arithmetic only**. The largest tested exception mixture, e=.30 with ceiling .535 and a3,000 principal discussion case, is the finite inactive nomination for root's independent economic review. It is not the most conservative possible model or evidence that either assumption is correct. The5,000 case is capital sensitivity; it does not enlarge any existing review or approve a second order.

The original ask was **.460 ×4,353**, source clock **2026-10-16T18:24:57.0360082+00:00**, receipt **18:24:57.162 UTC**, original receipt SHA256 `ba38de180cfd55768c2b1a981d10c178c6a78be5c886cd1a97211bc6c643ea32`. This was more than four hours old at bundle export. It is not executable now, not a backtest and not an observed fill. No cumulative depth beyond that old first ask is credited.

| Incremental principal illustration | Cash-only shares at old .460, ignoring depth | After10,000-share ceiling, still ignoring depth | With only old top depth | Principal at that top depth | Loss if ordinary Republican loss pays0 | Unused illustrative budget |
|---:|---:|---:|---:|---:|---:|---:|
| 3,000 | 6,521 | 6,521 | 4,353 | 2,002.38 | −2,002.38 | 997.62 |
| 5,000 | 10,869 | 10,000 | 4,353 | 2,002.38 | −2,002.38 | 2,997.62 |

Ignoring depth, those capped quantities would cost2,999.66 and4,600 at the old ask; they are **not available quantities**. With only the captured top-level depth, both budgets have the same2,002.38 exposure. A winning ordinary1/share outcome would increment that hypothetical principal by2,350.62; an ordinary loss or a zero-payout exceptional outcome loses the whole2,002.38. Fees are assumed zero only for these illustrations. Existing holdings/reservations must first reduce any genuinely available incremental/aggregate principal; no present flatness or cohort ownership is asserted.

The original official general-election field is **Aaron D. Ford DEM, Danielle Ford NPP, Joseph Lombardo REP**, plus **None of these candidates**. [Douglas](https://cltr.douglasnv.us/elections/2026-election-information/2026_general_election/who-is-on-my-ballot) and [Washoe](https://www.washoecounty.gov/voters/information/) original sources agree. Joe↔Joseph is explicitly a common-name identity inference, not equal source strings. [NRS293.269(2)](https://www.leg.state.nv.us/nrs/NRS-293.html#NRS293Sec269) excludes None from determining a winner; it is not a fourth winning candidate. Danielle is a real named NPP candidate, while the pinned CTM visible candidate-win table lists only the two major-party candidates. Full-field probability calibration remains unknown.

The exact original structured root SHA256 is `2f719ff73551d460ebac7077d3a59f11fa0f102bb17a06ef777b364b680a0541`, node receipt `13b18d2b206b4deed547ef4694dd5d261594c0de21889f0f9189d2cc565366eb`, received Oct16 at18:23:37.746 UTC. It says Party Winner / Republican Party / State / Governor of Nevada / General / Nov3,2026. Its scheduled Nov4 at17:00 UTC settlement is metadata, not assured certification or cash availability. A terminal hold is different from an earlier liquidation-value trade.

Before any future runtime review or dispatch, root must separately review nominee identity, full-field/model uncertainty, continuing-party/fusion/replacement and exact resolver/refund/cancellation semantics, original source expiry and approval pins. The sole worker must supply genuine current contract/root/book/depth/account/holdings/reservations/funding/caps and unknown-state checks; historical prices cannot replace them. This research changes none of those gates and supplies no physical authority. A hypothetical e value does not eliminate model error or correlate exceptions away.

Arithmetic was checked with exact `Decimal` and independently with rational `Fraction` calculations for all four ceilings, plus both principal/depth/quantity/loss cases. Minimal reproduction:

```python
from decimal import Decimal as D, ROUND_FLOOR
for e in map(D, ('.03', '.10', '.20', '.30')):
    ticks = ((D(1)-e)*D('.783')-D('.01'))/D('.005')
    ceiling = ticks.to_integral_value(rounding=ROUND_FLOOR)*D('.005')
    print(ceiling)
for budget in map(D, ('3000', '5000')):
    q = min(4353, 10000, int(budget/D('.460')))
    print(q, q*D('.460'), -q*D('.460'), budget-q*D('.460'))
```

Only this JSON and companion README are new. No SSH, account request, runtime, policy, approval or source clock was changed.
