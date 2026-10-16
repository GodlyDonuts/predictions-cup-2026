# Research papers: implementation and live evidence

Before drafting another paper, read [MOLOCH_RESEARCH_BASELINE](MOLOCH_RESEARCH_BASELINE.md) and pin the current Git commit plus the separately recorded deployed commit/binary and observation UTC. A code change on main is not automatically deployed. [CURRENT_STATE](CURRENT_STATE.md) is reverse chronological; older entries are history. Avoid treating an older supplied paper or dated candidate paragraph as a current capability list.

Papers are research inputs. They do not grant execution authority, establish current venue behavior or replace the dated production record in [CURRENT_STATE](CURRENT_STATE.md). Preserve a working sole production service while testing changes in isolated checkouts; deploy only checked changes through the existing account queue and ownership lock.

## Supplied October 15 papers

| Input | Archived source | Original attachment SHA256 | Main implementation/evidence |
| --- | --- | --- | --- |
| Paper 1: quantitative audit and latency roadmap | [Paper 1](../research/papers/20261015-paper-1.md) | `c23b1633b85a16afcddd3c1a74a3eebd0aa0d1aff91e482552376a985d43173c` | Rust production runtime; official own-fill accounting; [read planner](MOLOCH_RECONCILIATION_PLAN_20261015.md); actual fill proofs and separate inherited/new-trading results |
| Paper 2: relationship-aware portfolio edge | [Paper 2](../research/papers/20261015-paper-2.md) | `0124957712552edc8e3565e8c57f512a729679f7d3fc596f29e66a256723ace5` | [official feeds](MOLOCH_RELATIONSHIP_FEED_20261015.md), [payoff compiler/scanner/adapter](MOLOCH_RELATIONSHIP_ARBITRAGE_20261015.md), [atomic basket transport](MOLOCH_ATOMIC_BASKET_TRANSPORT_20261015.md), [live census](../data/moloch-relationship-census-20261015.json), [independent contract audit](../research/moloch-political-contract-proof-20261015/README.md) |
| Paper 3 and implementation brief | Original human attachments retained privately; [current implementation map](MOLOCH_PAPER3_20261016.md) | Paper `2bc7e8b055b3a8d1c7f33dfec719bad6a9ff573ad84d94f808a8c2ea3fae9d66`; brief `7004e8c4803b63d6d04042dab460b400af7cfaa7642d8e4afd52e099f968efe3` | Independent reviewed join/improve/bounded entries and original-proof cohort rearm deployed; first430YES forecast fill reconciled. Joint execution-distribution implementation exists but is disabled. [Actual economics](../research/moloch-actual-economics-20261016/README.md). Broader Paper2/3 research remains incomplete. |

Hashes identify original attachment bytes before Git newline normalization. Paper1/2 original sizes were56,136 and36,045 bytes; both were screened for concrete API-token/private-key patterns before archival. Credentials and private runtime account receipts remain outside Git.

## What changed because of the papers

The decision unit now has a source-bound portfolio/payoff representation with ordinary and exceptional resolution states, exact executable depth, funding/reservations, marginal inventory value and bounded orphan loss. The SDK supports the official relationship and account/collateral surfaces through the same serialized worker. A strict all-or-none basket transport prepares venue atomicity without enabling an unreviewed package executor. Ordinary single-order production remains subject to its existing authority.

The live census materially changed the research conclusion: at09:05:13Z on October15, all four surfaces returned HTTP200, but this tournament had zero relationships, zero constraints and zero collateral components. That is an available empty registry. The historical0.955 observation is not a current executable arbitrage. Matching canonical election race/stage metadata is useful, but independent void/refund and affiliation conditions must remain in the payoff model. The contract audit quantifies losses instead of inventing a guaranteed floor or exception probability.

Actual results also discipline strategy changes: the first inherited closing cohort realized2.340000 gross from78 shares, with official fill proof. At09:09Z total realized was2.760000, comprising2.870000 inherited gross and−0.110000 new trading. Inherited gains do not validate new strategy alpha. A process, order acknowledgment, public print, backtest or payout floor is never an actual fill or realized cash profit.

## Repeatable intake and promotion

1. Archive the supplied paper, record its hash/date and compare its stated repository/runtime baseline with current main and fresh production observations. Keep obsolete claims labeled historical.
2. Split claims into verified venue facts, mathematical results, assumptions and hypotheses. Check primary API/rule sources; test actual feature availability through the existing worker. Title matching and price correlation are discovery hints.
3. Create a task issue and claim exact paths. Map each proposed edge to its source requirements, payoff exceptions, execution mechanism and measurable outcome. Keep research edits separate from the running release.
4. Implement the smallest complete path with exact accounting, own-fill reconciliation, stable intents, realistic depth/latency and meaningful failure tests. Unknown acceptance, durable errors and ownership conflicts remain blocking; never reset their history.
5. Run matching Linux Rust tests/builds, preserve exact source and binary identities, publish checked commits directly to main, then perform a controlled rollout. Record process health, feed continuity, official orders/fills and economic results separately. Retain the existing funded bank and rate history.
6. Report negative results and unavailable features alongside successes. Promote an expected-value hypothesis with independent evidence; do not equate a test count, transaction count, collateral advance or inherited gain with a profitable new strategy.

Current task threads: [production #120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120), [feeds #123](https://github.com/GodlyDonuts/predictions-cup-2026/issues/123), [payoffs #124](https://github.com/GodlyDonuts/predictions-cup-2026/issues/124), [read reduction #126](https://github.com/GodlyDonuts/predictions-cup-2026/issues/126), [atomic baskets #127](https://github.com/GodlyDonuts/predictions-cup-2026/issues/127) and [queue maintenance #128](https://github.com/GodlyDonuts/predictions-cup-2026/issues/128). Each task records source, validation, deployment status and remaining uncertainty.
