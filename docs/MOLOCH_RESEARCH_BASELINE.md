# Moloch research baseline

**As of2026-10-16 04:33:41UTC.** This is the starting point for new papers. It describes the actual deployed bot and separates source-only work. Observations age; consult the newest [CURRENT_STATE](CURRENT_STATE.md) entry and linked evidence before making a current live claim. Older dated documents are historical unless this baseline identifies them as current specifications.

## Exact deployed system

| Item | Verified state |
| --- | --- |
| Service |Sole `moloch-live.service`, PID381131, VPS; running, no execution STOP|
| Deployed source |`4148fe5dd3bb1bd4c1ba53855ab1422d80d6f61d`|
| Executable SHA256 |`da4202e064ffb89cac1eb4aab2dcb076a1d2bcc3cde3c1a2197799f6cf9c5fefc`|
| Native engine identity |`4b50ec49ba7331eb051e7983e33ffa0ce55df19b9a9e4a7d9d5d8d8a5477ff98`, nine explicit financial identity bridges|
| Account/bank |One serialized SDK V1 queue and original funded bank; no second collector/writer|
| Fresh activity |Healthy realtime/trading;696 total acknowledged orders,119 actual fill rows;14 new ACKs/1 new fill since03:58 recovery launch|
| Risk limits |40,000 total commitment;10,000 per market;10,000 shares maximum order; existing holdings separately attributed|
| Read/write limits |100-read rolling ceiling/600ms completion gap;24-write ceiling/2500ms gap. These are admission ceilings, not measured achieved throughput|
| Entry schedule |Human-authorized daily windows; current cutoff14:44:34.413UTC October16, fixed campaign end17:00UTC November4; exits continue|
| Ordinary policy |Paired-spread maker, one-tick minimum spread,.001 minimum edge,.005 fixed-gain exit; execution-clock maximum inventory age **60,000ms**|
| Optional features OFF |Execution-distribution diagnostic, old execution-model diagnostic, empirical maker-policy gate and collateral accounting|

[Actual immutable rollout and stopped boundary](../research/moloch-permit-recovery-20261016/production-rollout.json), [latest checked live observation](../research/moloch-permit-recovery-20261016/live-verification.json). Checked `main` may advance beyond deployed source. A source commit, test or documentation edit does not establish deployment. No runtime change was made for this documentation refresh.

## Capabilities that exist now

- Independent reviewed forecast origination with real native funding, fresh final official facts and exact physical capabilities. Explicit reviews support join bid, improve one tick and bounded marketable limits with a possible resting remainder. Ordinary maker BUYs retain their passive rule.
- Actual own-order receipts, partial fills, fees, duplicate detection, cancellation races and exact FIFO accounting. Public prints and provisional marks never credit a fill.
- Reviewed forecast holdings/value exits/fixed deadlines, plus separately verified inherited-origin management. Neither blindly adopts holdings nor resets historical costs.
- Original-proof closed-zero and never-attempted forecast abandonment/rearm. No same-key retry, timeout-based closure or implicit uncertain-order replacement.
- Official relationship/payoff/atomic-basket infrastructure and discovery exist. An empty relationship registry is not a profitable basket; no unverified payout floor or package activation is implied.

[Forecast specification](MOLOCH_FUNDAMENTAL_EXECUTION_20261016.md), [runtime operation](../moloch/live-runtime/README.md), [Paper3 map](MOLOCH_PAPER3_20261016.md).

## Actual economics and current position

The restored worker bought430 Delaware Democratic Senate YES1042 at.920, principal395.600000, fee0, executed04:02:17.490UTC, order41846939/q3420. It remains open; realized profit from this position is0. Review9523497c… assumes.95 ordinary probability/.01 exceptional allowance/.01 margin, giving.9405 expected payout and.9305 ceiling before tick flooring. New principal cap400; source/review validity ends09:00UTC October16; original holding deadline17:00UTC October19. Model assumptions do not guarantee a favorable exit by that deadline.

The actual sealed FIFO audit reconciles119 unique fills/38 duplicate original events and all final cash, positions, costs and fees. New ordinary trading realized−119.100000, inherited gross+43.395000, combined−75.705000. All30 closed ordinary acquisition lots exited through maximum-age orders;953YES and885NO account for91.82% of the loss. This warrants evaluating entry adverse selection and exit economics; it does not prove that disabling the timer or excluding those markets would improve future returns. [Audit and exact source boundary](../research/moloch-actual-economics-20261016/README.md).

Cash17,046.770000 and owned cost22,877.525000 are attributed-bank measures at the linked observation. They are not whole-account equity or marked liquidation value. The human's earlier official daily dashboard+1,168 observation is separate and differently dated; it is not the new-entry realized measure above.

## Unfinished work and research targets

Paper2 and the broader Paper3 strategy are incomplete. No profitable competitive production strategy has been demonstrated by the results above. The joint zero/partial/full execution-distribution implementation is checked but currently disabled after its synchronous scan caused a production permit-expiry stop; it is not a calibrated active maker selector. The scan and incident were repaired, original no-send evidence retained, and production restored. [Incident](MOLOCH_PERMIT_RECOVERY_20261016.md).

External-source catch-up, local dislocation, probabilistic joint baskets, genuine partial-return vintages and terminal-wealth optimization remain research hypotheses. Do not repeat earlier claims that forecast entries depend on spread proposals, can only join bid, or cannot rearm after proven zero fills. Conversely, do not infer a complete strategy, calibrated uncertainty, active collateral/package trading or production queue V2 from modules that merely exist on main.

Rhode Island official September primary nomination evidence is retained, with its reported358/402 precinct limitation. It does not establish current general-ballot continuity, full resolver semantics or a new RI activation. Massachusetts7061YES has a separate inherited-management review and no extra-buy authority.

## How to base the next paper

Pin `git rev-parse HEAD`, this baseline's observation UTC and the separately recorded deployed source/binary. Read [PAPER_INTAKE](PAPER_INTAKE.md), the current capability specification and actual economic evidence. Label each proposed improvement as already deployed, checked but inactive, or missing. Use official semantics and executable depth; separate assumptions, counterfactual outcomes, actual fills, attributed realized P&L and dashboard marks. Include the precise files/behavior to change and an attributable economic outcome to measure. Task evidence: [production #120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120), [Paper3 #144](https://github.com/GodlyDonuts/predictions-cup-2026/issues/144), [incident #145](https://github.com/GodlyDonuts/predictions-cup-2026/issues/145), [actual economics #146](https://github.com/GodlyDonuts/predictions-cup-2026/issues/146).

Current checked but inactive performance work: queued-frame source batching d1e15194 passes38 worker checks and its ARM64 release build; an original-prefix serial/batch benchmark is still running. This is not the deployed service. [Measured cycle-latency work](../research/moloch-live-latency-20261016/README.md).
