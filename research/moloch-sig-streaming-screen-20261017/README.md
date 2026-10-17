# Frozen SIG-only streaming screen

Task166, research only. This reader consumes original admitted native book/trade/gap rows without Session replay, account calls or a new collector. Every output remains UNQUALIFIED_RESEARCH/executionAuthority=false/modelApproval=false, whatever its score. Root owns original export, actual-run review, publication and execution.

## Predeclared experiment

Protocol SHA is pinned in analyze.py. Fixed60s nominal hold, first valid newly observed book per exact tournament/market/exchange and60s processed-time block. The first valid book consumes its block even if features are missing; no alternate rescue. Only one pending crossing per scope. Features are microprice-minus-midpoint, imbalance, spread and causal1/5/30s midpoint changes. Lag anchors must have been processed before decision-minus-lag and be within20s anchor slack. Gaps clear histories and censor pending samples. Original source/request/receipt and processed first-availability clocks stay separate.

Optional unsigned30s volume is diagnostic-only, never a model column. It requires actual complete coverage of the whole execution-time window and sufficient projected sequence. Missing volume/anchors are not zeros; known complete empty volume can be zero. No aggressor is inferred from native public trades.

Three equal-duration calendar test quarters follow the first training quarter of the ORIGINAL suffix processed-time span. Fit only earlier completed labels, with actual availability before each test boundary and purge through the60s horizon, whole virtual exit delay and3s slack. Train-only scaling, ridge1,16-training minimum. Hold out SHA256(groupID) modulo5==0 from every training fold. Without a pinned semantic group map, use marketID and explicitly report semantic-group-unavailable. Related election risks remain correlated.

All comparators share the identical complete cohort and folds: unchanged original BBO, local imbalance/spread ridge and six-feature ridge. Separate YES/NO targets; fixed positive-predicted-net selection. Net values deduct zero ASSUMED fees and one fixed extra tick(.005); no current fee authority is asserted. OOS and selected minima are each100. Count support is not model approval. Exact sums use integer millionths, with floats confined to regression/features/means. No threshold/horizon search.

## Counterfactual pacing and marks

One serialized virtual queue:100reads/24writes per rolling minute,600/2500ms completion spacing. Each entry/exit packet costs7read slots plus1write; first read also waits600ms and entry respects at least3s activation. Packet delay beyond10s is unavailable. This explicitly assumes zero HTTP duration and no competing account load/Retry-After/unknown/funding state; completion spacing is not latency. A failed packet probe spends no virtual slots.

The first original book after readiness must be fresh/nonfuture, source at least readiness, observed within3s. No later favorable replacement for an invalid/stale/late/depth-limited first book. YES buys use asks and sell marks use bids; NO uses1-bid and1-ask respectively. One whole share must fit after subtracting suffix-known own ACK remaining quantities, updated by actual fills/terminals. Pre-anchor own-order state is unavailable, so displayed depth is optimistic, not guaranteed own execution. Owned fills/public trades are deduplicated; public identity memory fails at32768 rather than evicting and renewing clocks.

The60s target starts at the hypothetical entry-book OBSERVATION, then the exit needs another queued packet. Output gives the actual observation-to-label duration; it is not an exact60s actual fill return. Missing/censored marks are not flat returns. Marked sums are not portfolio profits or jointly executable trades. The existing execution_edge/execution_distribution own-fill observers keep their stronger Session/attempt/terminal boundaries and are not replaced.

## Input and reproduction

```
python -m unittest discover -s research/moloch-sig-streaming-screen-20261017 -p test_analyze.py -v
python research/moloch-sig-streaming-screen-20261017/analyze.py --manifest PRIVATE_INPUTS.json --manifest-sha256 ORIGINAL_RAW_SHA --output NEW_PRIVATE_REPORT.json
```

SchemaVersion1 manifest: sealed:true plus original journal,anchorHead,report,bankManifest pins, each exactly{path,sha256,bytes}. Relative paths resolve beside the manifest. Optional finalHead/boundary/anchorBoundary/semanticGroups pins are also verified; semanticGroups is marketID->reviewed office/event group. Boundary pins remain owner-provided evidence, not inferred authority. Require anchor.sequence+1/anchor.previous through exact final report sequence/previous/processed clock and matching bank identity. Original stopped-head authenticity comes from the separately authenticated root binding, not self-hashing a file.

Verify all full bytes/hashes/regular-single-link identities BEFORE outcomes, then again afterward. Reuse the checked raw native seal/hash/depth parser (preserves original numeric lexemes) and fixed ridge linear solver read-only. The small fitting adapter applies the same train-only scaling/ridge1 to the declared dimensions. No financial replay or underlying raw sourceRef body membership claim follows. Exact input/protocol/readers/source hashes remain in output.

Bounds:2GiB journal/1million rows/16MiB row/1024scopes/2048scalar history points per scope/1024pending/50000examples/32768public IDs/50000owned identities/1024active known own orders. Three fixed fitting folds and constant-bounded histories/scans keep finite processing linear, without per-event financial-state clones. Output is create-new; raw books/orders/fills remain private. Source failures reject rather than minting successful results.

Original outcomes were not examined while implementing/freezing this reader. Prior zero-after-cost v9 selections/nonpositive sparse owned-maker marks remain unchanged. This is one new fixed prospective-style offline diagnostic, not a retuning of those results.
