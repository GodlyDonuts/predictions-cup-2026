[Later checked public-trade path and queue](public-trade-stream-20261019.md). Earlier queue/source observations below are historical.

# Correct the handcrafted maker probe before interpreting its economics

The previous handcrafted baseline anchored its refresh timer before reads, planning and write receipts. It could consume that entire interval before submitting quotes, then immediately start cancellation. Its zero fills therefore did not establish a passive-execution failure or absence of maker opportunities. This flaw is in the separate diagnostic harness; the neural collector and simulator clock were not changed.

The longer005probe used96 paid book-baseline attempts per refresh, two paid account resources, cancel/submit operations and charged planning. At the frozen600ms REST latency these operations exceed its nominal60-second cycle. The earlier004five-second/six-book probe had the same timing risk. Per-leg acceptance failed to reveal this flaw. Existing artifacts remain preserved and are not credited as evidence against market making.

The007posthoc audit reran the first900-second TRAIN case with the same policy. All650 legs were accepted, with50open at acknowledgments, and211exchanges quoted at some point. Nevertheless, none of109recorded prints occurred during *any* live quote. Twenty-four of29traded exchanges were quoted at other times. A read-only cache query found every traded exchange in the237-exchange book catalog, excluding an exchange-ID mismatch as this probe's explanation.

## Corrected finite experiment

008anchors a60-second wait after processing and the quote receipt, retaining the original60-second expiry measured before planning. Quotes rest until their actual expiry rather than being cancelled immediately. No engine fill rule, price rule, quantity, source clock, API budget or historical row changes. Audit diagnostics do not enter decisions.

The two already-inspected TRAIN windows are900seconds: `1792410601000–1792411501000` and `1792412401000–1792413301000`. Each runs nominally and with existing half-depth/0.005 adverse-execution stress. These are repeated scenarios over two calendars, not four independent days. Delivered books, paid positions/account observations and public/local state inform orders; physical order state and original prints are posthoc diagnostics only.

| Case | Accepted legs | Prints during any live quote | Prints in exchange with own live quotes | At own limit | Strictly through own limit | Fills / marked ROI |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| First, nominal |350|36|6|3|0|0 /0|
| First, stressed |350|36|6|3|0|0 /0|
| Second, nominal |350|50|2|2|0|0 /0|
| Second, stressed |350|50|2|2|0|0 /0|

This establishes actual quote exposure and poor concurrent activity coverage for this policy. Equal-price prints alone do not establish that displayed queue-ahead should have cleared. It does not prove simulator calibration, that no maker strategy can profit, or that an edge exists. No policy was adopted as an expert, promoted, deployed or used for neural gradients. These are development midpoint/log-wealth results, not independent daily-growth qualification.

All finite runs used frozen020source544a3986, CPU-only environments and dataset7a85747f768652c8fea1fa2cd1647eac5549b0274b9f623d023da084b70de07c. They create no HTTP requests or official account clients. The first005launch failed at import before creating a study directory; its failed log is preserved/uncredited. The corrected import path points to the unchanged frozen source.

## Exact evidence

- [005 longer probe](delivered-paired-maker-train-probe-005.json), [exact runner](delivered-paired-maker-train-probe-005-runner.py.gz).
- [007 coverage audit](delivered-paired-maker-train-probe-007.json), [exact runner](delivered-paired-maker-train-probe-007-runner.py.gz).
- [008 corrected waiting anchor](delivered-paired-maker-train-probe-008.json), [exact runner](delivered-paired-maker-train-probe-008-runner.py.gz).

Gzip preserves original runner bytes and line endings; decompressed SHA256 matches each report's `runnerSha256`. Runtime source pins are included. Repetition adds no historical coverage or independence.

## Next information-fidelity gap

Current `ApiEpisode._event('trade_flow')` updates server-side `trade_history`; only paid `api_trade_receipt` adds prints to actor-visible `flow`. `DerivedStream` forwards books and private events but has no public-trade delivery path. The cached official October19OpenAPI `/realtime/token` description (canonical SHA256fbdf23e54d317392f9fb4a659b9511a01fad065e7a384f28091ef04894a732c9) includes public trades in authorized market batches. The current derived adapter therefore omits this information path.

A future explicit profile should forward only original validated prints at recorded availability while the simulated connection/token is active, preserve duplicate identity/facts across REST and stream, and suppress delivery across known gaps. It must not invent raw revisions, backfill unseen history for free, reveal executions before receipt, or claim raw-topic parity. Active020and queued021sources remain unchanged by this audit. Original [SIG v1 API](https://sig.thesuper.market/api/v1/docs); no fresh live-schema verification is claimed.
