# Genuine primary-input refresh â€” 2026-10-16

This is a **new two-race research artifact**, not renewal of the expired four-race artifact. No existing reviews, controls, trading values or activation were changed. No SIG/account requests or production changes were made.

Artifact model SHA256: `2c2e3492cce8ae1d52a35ef476f5905f6990d3da865382e99c481c0de38d4e69`. Original shared receipt window: **2026-10-16T08:04:01.616Z to14:04:01.531Z** (six-hour research receipt TTL). Four joint ordinary states; integer weights sum1,000,000. This artifact is research-only and cannot mint an execution context. Two races do not satisfy the existing three-distinct-race curve requirement.

## What actually changed

New original public data were found that were **not in the previous Atlas input**, with later fieldwork than the old Emerson polls. These are not claimed to be new publications after today's07:12 expiry. The source publication clocks are not established by the PDFs; HTTP Last-Modified is retained as server metadata, not relabeled publication time. Original availability means this task's actual first receipt of these distinct data.

|Race|New original source|Actual field dates|Actual LV sample|Named Senate responses|Original receipt UTC|Raw SHA256|
|---|---|---|---|---|---|---|
|Iowa|[YouGov original PDF](https://d3nkl3psvxxpe9.cloudfront.net/documents/ttw_ia_20261006_lv.pdf)|Oct6â€“12|2245|Josh Turek47 / Ashley Hinson46|08:04:01.531898|86968315dabd82f898119fa650f277257dce2e37915cdea0772a3d4c4d4090d9|
|Kansas|[YouGov original PDF](https://d3nkl3psvxxpe9.cloudfront.net/documents/ttw_ks_20260921_lv.pdf)|Sep24â€“Oct4|2255|Adam Hamilton48 / Roger Marshall45|08:04:01.616724|2f9f21d7b589b6dbeefe80a7370a7aecb7b0fb4085a704d2dc37b99c04cf7062|

Both are linked directly by the captured [official YouGov survey index](https://yougov.com/en-us/survey-results). First-page scope/sample/field dates and second-page named-candidate Senate results were extracted locally; the original second pages were rendered and visually checked. Do not derive Kansas field dates from its misleading filename: the actual printed interval is Sep24â€“Oct4. The question allocates undecided preference; differences from Emerson's unallocated results are not a pure temporal trend. Same-survey governor outcomes are excluded to avoid assuming unknown cross-office measurement covariance.

The [official Iowa general list](https://sos.iowa.gov/sites/default/files/2026-08/2026%20General%20-%20Candidate%20List%20Database%20-%20All%20Elections_1.pdf) and [official Sedgwick Kansas candidate table](https://www.sedgwickcounty.org/elections/current-candidates/?page=1) were independently re-fetched at08:07Z and are byte-identical to the earlier retained official sources. Names/party/office match the new polls. This is current re-verification, **not new ballot information**, and does not renew any poll input or existing review. Kansas county rows are not asserted exhaustive; the source list missing a third-party row does not prove absence.

Marist's [Oct15 Iowa release](https://maristpoll.marist.edu/polls/2026-midterm-elections-in-iowa-october-2026/) reports Turek48/Hinson45 among likely voters, fieldOct8â€“11. Its original download was blocked (initialHTTP403, laterDNSfailure), so it is excluded from the artifact. The1,100 sample is registered voters; it was never substituted for the unpublished likely-voter count. Search-rendered evidence is not retained original bytes.

No verified later NC/NY primary releases were obtained. Their old source clocks and probabilities were **not** refreshed or copied into this artifact. The existing NC reviewed probability.919/ceiling.88143 remains distinct from the expired Atlas.975236 estimate and is unchanged.

## Valuation and sensitivity

The original checked historical fit was reused only after its exact canonical parameter hash matched the previous artifact's declared fittedParameterSha256. It uses416 actual historical races/1291 pollster-race observations; no model selection or refit was performed to seek a favorable result.

|Ordinary Democratic event|Previous expired Atlas MC pD|New MC pD|Analytic pD under âˆ’3/+3 margin-point shared bias|Analytic pD with covariance doubled|
|---|---|---|---|---|
|Iowa Senate|.297257|.496017|.270110 / .727458|.498882|
|Kansas Senate|.500870|.578751|.355347 / .772797|.556637|

Monte Carlo marginals differ slightly from analytic values (.498420IA/.579718KS) due to finite joint draws. `diagnostics.json` also includes Â±5-point and doubled-covariance plus bias stresses. These are explicit sensitivity assumptions, **not calibrated confidence intervals or certified lower probabilities**. Nominal sample sizes are real, but effective design sample sizes are unknown; no MOE-derived synthetic count was invented. Common YouGov house uncertainty is retained jointly across the two state races.

Held-out evidence remains the previous actual leave-election-cycle-out validation:416races/23cycles, Brier.08437988 versus pooled.08448569, log loss.26609312 versus.26337861 (worse), marginRMSE7.63227 versus7.45931 (worse). The23-cycle paired Brier bootstrap95% interval[âˆ’.00416476,+.00474721] crosses zero; superiority/profit is not established. Historical availability/population gaps, current horizons outside historical training support, unknown design effects and unidentified exceptional/refund probabilities keep qualification at research.

## Opportunity shortlist and price limits

1. **Iowa Democratic Party YES, market260/exchange949/contract261:** new primary information moves this model toward even odds. This invalidates treating the expired model's strong Republican lean as current model support. Before any fees/exception/capital charges the new point break-even is about.496; a âˆ’3-point margin stress gives only.270. A current executable quote is required to distinguish mispricing from ordinary forecast uncertainty.
2. **Kansas Democratic Party YES, market356/exchange1045/contract357:** new source point value about.579; âˆ’3-point margin stress about.355. This is a conditional research candidate, not a robust positive-edge claim. Previously retained catalog price.41 is old and must not be used as today's ask.

Parent's bounded retained-journal scan subsequently found latest normalized sourceAt: IA949 **2026-10-15T13:53:31.821988800Z**, KS1045 **2026-10-15T19:06:32.840430000Z**, NC966 **2026-10-16T08:07:18.774655800Z**. IA/KS are stale by about18/13hours at this research observation; the current NC book does not renew an excluded NC model input. Parent retained exact prefix/event/sourceRef/row-hash evidence in private `/tmp/moloch-research-book-tail-20261016.json`; these are derived native book events, not original HTTP receipts or physical authority. **Current executable IA/KS opportunity/size is unavailable**, not inferred from old last/catalog prices. No book/account client was created. Any later comparison must retain request/received/asOf clocks, full depth, original contract/refund semantics and independently approved valuation/risk bounds. No price or risk-policy change is proposed from these sensitivities alone.

## Reproduction and verification

Original PDFs, receipts and rendered pages remain private under `Temp/moloch-atlas-primary-refresh-20261016`; `source-inventory.json` retains exact paths, URLs, hashes, status and source clocks. Original cached training files remain under `Temp/moloch-atlas-primary-20261016/fit-v3`. The old four-race originals/artifact remain untouched.

`refresh.py` performs no network access. It pins exact original PDF and original receipt hashes, rejects clock editing, checks explicit named Senate pairs/sample scope, checks the existing fit identity, emits only into a new absolute private directory and refuses an expired original window. Its CLI requires `--sources`, `--fit`, `--validation`, `--prior` and `--out`; the prior artifact is used for fitted-parameter identity only, never as new observations.

Four actual-source tests passed: exact two-poll scope/names/sample/TTL; original receipt retimestamp denial; altered PDF denial; existing output overwrite denial. Artifact self-hash and integer scenario sum were independently recomputed. No Rust/build/runtime/account tests were needed or claimed for this offline source update.
