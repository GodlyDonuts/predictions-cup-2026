# Owned-order acquisition and markout audit

Source/data-only handoff `owned-fill-calibration-20261017`, task [#162](https://github.com/GodlyDonuts/predictions-cup-2026/issues/162). The protocol was declared at **2026-10-17 06:56:36 UTC**, before the private outcome audit. No collector, account request, execution authority, fitted model or activation is added.

The frozen bank boundary is **426,656 events / 355 unique actual fills**, at **2026-10-17 06:30:06.261 UTC**. The private extraction contains 312,276 original normalized book-event rows, the original full dispatch prefix, and original order/fill receipts. A separately pinned native subset supplies ACK, terminal, adoption and fill membership. The full financial journal is not included or independently replayed here.

The placed denominator is **1,502 orders**: **1,299 confirmed zero, 26 partial and 177 full**. Zero requires a genuine original closed order, a later complete scoped fill tape, matching native terminal, and the original durable `closed` tuple. Never-sent quotes and no-network attempts are excluded. Eight later ACKs outside the native order prefix are excluded. Two absent receipt files belong to one recovered order; the same original order/tape are preserved inline in its sealed dispatch record and matched to the native receipt references. They are not reconstructed or invented.

`summary.json` separates ordinary BUYs, ordinary SELL exits, and reviewed forecast modes. It preserves requested physical quantity separately from ACK-admitted quantity and native quote quantity. These are historical closure-conditioned fractions under changing entry/cancellation policies, **not** a calibrated probability that a new passive order fills. Immediate executions during the POST response are separated from executions strictly after it; neither is asserted to be an official maker/taker classification.

| Original entry class | Placed | Zero / partial / full | Actual filled shares | Execution timing |
|---|---:|---:|---:|---|
| Ordinary BUY | 891 | 850 / 23 / 18 | 45,511 | All 75 fill rows strictly after the POST response |
| Reviewed `boundedLimitMayRest` BUY | 26 | 0 / 0 / 26 | 87,381 | 84,620 shares before/during response; 2,761 after |
| Older reviewed BUY, mode unavailable | 1 | 1 / 0 / 0 | 0 | No fill |

All 26 bounded-limit forecast placements were **marketable against their original pre-request book**. Their 26/26 full outcomes cannot train a passive fill-probability assumption. Among ordinary BUYs, 883 were classified joining the side bid, two inside the spread, and six lacked adequate causal feature/POST evidence. Only 41/891 ordinary orders obtained any fill, with cancellation and expiry selection retained.

For the 75 ordinary BUY fill rows, original external-depth labels cover **12 / 12 / 18 / 14 rows** at 1/5/30/120 seconds, respectively. Covered shares are **10,671 / 12,329 / 20,277 / 13,906**; the corresponding displayed-depth diagnostics before unknown exit fees are **−59.765 / −63.425 / −80.840 / −53.595 SUSQie**. These horizons contain different observed subsets, so they do not establish that waiting longer improves or worsens a strategy. Most early labels were available before native fill discovery, further separating offline diagnostics from an actionable worker response.

Features use only original information available strictly before the original POST request start. Recovered GET ACKs cannot invent a missing POST decision clock. Native commit-order adoption/fills supply known opposite holdings; unavailable reservation evidence prevents calling them fully covered hedges. Known still-open own depth is subtracted using earlier native ACK/fill/terminal evidence. Cancel ACKs do not release it. Uncertain depth remains unavailable.

At each preregistered 1/5/30/120-second horizon, the first original same-scope book source in the following five-second window is assessed at full actual fill quantity. A late, stale, invalid or shallow first candidate cannot be replaced by a favorable later one. NO liquidity uses the exact complement of the YES book. Original sequence, engine time, receipt time and native processing time stay separate. Missing and right-censored labels are not zero.

BUY labels are displayed external bid proceeds minus actual acquisition principal and recorded entry fee. SELL labels compare actual proceeds with a hypothetical same-side repurchase at the later ask. The latter mechanically includes spread cost and is **not** realized inventory profit or evidence that an exit was wrong. Future exit fees remain unknown. Short-horizon forecast BUY marks do not estimate terminal value. No party/candidate semantic correspondence is inferred from a YES/NO book.

The predeclared economic-coverage gate is not met, so no chronological predictive model is fitted. Hundreds of time blocks and many closure outcomes cannot replace absent executable labels or missing causal reservation evidence. This packet supports acquisition/coverage diagnostics, not a profitable maker or terminal strategy claim.

## Reproduce

Use Python 3.11+ standard library, with the repository helper `research/moloch-owned-fill-markouts-20261015/analyze.py` present. The auditor pins its raw-byte hash in the result. Private inputs remain outside Git. The source ZIP hash is `59bb41ed83344365b6dd8fd924f60f29ee71e8140992176d839d856c8cdd2db3`; the native companion ZIP hash is `b28ad181923da532f327c3178c1072f8e04bd7eaffe957cf7d5699438cc3d10a`.

```text
python -B -m unittest discover -s research/moloch-owned-fill-calibration-20261017 -p test_analyze.py -v
python -B research/moloch-owned-fill-calibration-20261017/analyze.py --input PRIVATE_INPUT --source-manifest-sha256 7547749cc1c4fad9cd109116b0976a9544dffc648e086d5de8071d3ae3b0530e --native-membership PRIVATE_NATIVE_COMPANION --native-membership-sha256 f54dffce927c914db944a1a9c0f801c92a14c2d6e30d070d7a93996046d3b5bf --output NEW_SUMMARY.json
```

The output path must not exist. Wrong pins fail before analysis/output. Every provided file's raw hash is checked, dispatch row seals/full chain are verified, and individual native-subset row seals/adjacent links are checked. The extraction's full-prefix membership is an owner-provided pinned source statement, not recreated financial replay. Python does not recreate Rust's typed `Receipt` digest from pretty JSON: raw file hashes, original native references and original dispatch lineage remain distinct.

Source-code fingerprints are explicitly Git-LF SHA-256 (CRLF-to-LF for code only); original data bytes are never normalized. Twelve focused adversarial checks passed, covering terminal completeness/causal ordering, canonical scoped endpoints, NO signs/depth complements, strict pre-request features, first-candidate missingness, future own-fill exclusion, own-liquidity retention until terminal, and wrong source pins. The full actual audit verifies all 1,502 ACK/terminal bindings and all 355 native unique fills; 108 later repeated fill events retain the same economic identity and are not double-counted.

The smallest next measurement uses the **existing** raw stream/breadth capture and independent acquisition observer: retain continuous original versioned book/depth availability around placed attempts and fills, plus original terminal/tape pairs and causal owned-liquidity state. Do not add reads or renew quiet-book clocks. First measure label coverage by release/market/time and immediate/rested status; fit an earlier/later model only after its economic labels satisfy the frozen gate. Current gaps cannot justify a longer hold, more writes, or more capital at the same entry rule.
