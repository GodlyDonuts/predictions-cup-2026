# Retained Atlas contract bindings — 2026-10-16

`live-binding-inventory-20261016.json` is an evidence inventory for diagnostic mapping. It grants no execution authority and changes no existing review.

| Atlas race | Market / exchange / contract | Explicit original predicate | Node received UTC |
|---|---|---|---|
| 2026:senate:NC | 277 / 966 / 278 | NC Federal, General, Party Winner, Democratic Party, race62975/stage98105 | 2026-10-16 05:54:07.744 |
| 2026:senate:KS | 356 / 1045 / 357 | KS Federal, General, Party Winner, Democratic Party, race62962/stage98092 | 2026-10-15 19:08:55.461 |
| 2026:senate:IA | 260 / 949 / 261 | IA Federal, General, Party Winner, Democratic Party, race62961/stage98091 | 2026-10-15 13:31:27.356 |
| 2026:governor:NY | catalog locator184 / 873 / **unknown** | **original node missing** | unknown |

Each verified node contains explicit `contract_id`, `contract_type`, `contract_details`, tournament context and original scoped endpoint. Exchange IDs come independently from original catalog rows, not node titles. All three verified predicates specify electionDate2026-11-03 and scheduled settlement2026-11-04T17:00:00.000Z; `settled_with` is null. Neither schedule nor null establishes exceptional/refund rules or actual future payout.

The raw receipt SHA256s match the parent-supplied retained archive index. Semantic root SHA256 uses UTF-8 sorted-key compact JSON over the **whole original data.root**, with no title/field removal and no newline. Original request/received clocks remain unchanged. Catalog line hashes exclude the line delimiter; whole-file hash is separate. The inventory retains historical derived Kansas/Iowa lookup hints but does not promote their hashes to original proof.

No account requests were made. Parent copied the three existing originals to private `Temp/moloch-paper5-binding-nodes-20261016/nodes`. The earlier NC23:45Z original has the same root but different receipt bytes; the new inventory uses the actual05:54Z receipt, preserving its distinct raw hash. Original bytes stay private.

A diagnostic consumer may explicitly bound semantic age to24h; this is a reviewed diagnostic convention, not fresh physical authority. Fresh books/account/rules, explicit ordinary-scenario/party mapping, exceptional-state handling and separately approved model-specific reviews remain required. The current Atlas surface is research-only, ordinary-outcome-only and expires07:12:46.963Z onOctober16. Its NC probability.975236 does not replace the existing NC review probability.919.

Validation performed: all three raw hashes vs retained index; exact endpoint/market/tournament context; status200; explicit node_id=contract_id; contractType Election Outcome, resolutionType Party Winner, raceStage General, winnerName Democratic Party; canonical root hashes rederived. No runtime/build/API tests were run because only evidence documentation changed. NY requires an original `/markets/184/nodes?tournamentId=…` receipt before binding; Republican185/874 also lacks one locally.
