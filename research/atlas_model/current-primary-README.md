# Atlas current primary-source input — October16

This is a new current input, not the expired October5 transcription with a changed clock. Four original publisher/commissioner documents and their ORIGINAL receipts are hash-pinned. The existing Atlas model is unchanged. The exporter verifies the exact source bytes, URL/status, receipt bytes, publication metadata, extraction anchors and source clock order before fitting the original416-race training set and emitting sixteen correlated ordinary joint states.

The current research interval is **2026-10-16T05:41:40.656Z through07:12:46.963Z**. Its end is the earliest actual receipt plus the declared six-hour research TTL, Kansas01:12:46.963Z. Recapturing a timestamp in the old receipt is forbidden: original receipt hashes are also pinned. After expiry the current exporter withholds a new artifact until genuinely new source evidence is obtained and reviewed; it does not renew the original window.

| Exact race ID | Primary observation | Actual nominal LV sample | Original field dates |
| --- | --- | ---: | --- |
|2026:senate:NC |SurveyUSA/WRAL54D,40R |643 |October7–11 |
|2026:senate:KS |Emerson45D,43R |750 |September15–16 |
|2026:senate:IA |Emerson45D,47R |650 |October5–6 |
|2026:governor:NY |Emerson52D,40R |900 |October3–5 |

The original [WRAL commissioning release](https://www.wral.com/news/nccapitol/wral-nc-us-senate-poll-cooper-whatley-tillis-trump-stein-approval-midterm-october-2026/) distinguishes643 likely/already-voted respondents from900 total respondents; the model uses643, not900. [Kansas](https://emersoncollegepolling.com/kansas-2026/) is a genuine older field window, not an October poll. The [newer Iowa release](https://emersoncollegepolling.com/iowa-2026-poll-u-s-senate-race-tightens-sand-maintains-edge-for-governor/) and [New York release](https://emersoncollegepolling.com/new-york-2026-poll-governor-hochul-leads-blakeman-by-12/) were captured as original HTML at05:41:39/40UTC.

Normal local DNS failed for Emerson. A retained original Google public DoH response resolved104.197.152.65. Acquisition used that address with standard CA verification and TLS SNI/HTTP Host `emersoncollegepolling.com`; certificate verification was not disabled. Receipts preserve the IP, verified hostname, resolver URL and original DNS-response SHA. This is a one-off public source capture, with no SIG/account request or collector. Raw publisher pages, PDFs, receipts and DNS response remain private local source evidence; only structured observations and aggregate artifacts are committed.

Publisher `datePublished` is parsed from original structured metadata. Actual first-known availability is the original request/receipt interval, not the earlier publication/field time. Original field dates have no intraday timestamps; `fieldAtMs` is an explicitly labeled midpoint-of-date/noon encoding for the model, not an invented observed field timestamp. Nominal sample sizes are known, effective design effects are not. Pollster names are not guessed to match historical rating IDs; all current Emerson inputs share the same new publisher effect/uncertainty.

The ECU original topline PDF and official release disagree by one day on their field dates. The PDF lists September29–October2, while its release says September30–October3. Q5A retains undecided respondents49.5D/42.8R; forced-choiceQ5B52.5D/45.5R uses the same810 people. ECU is withheld by default rather than choosing a convenient version or double counting those questions. Same-survey Kansas/Iowa governor questions are also withheld: the model has no respondent-level cross-office measurement covariance. CTM/99chambers probabilities are forecast outputs, not polls, and are not converted to fabricated poll margins.

[Current primary inputs](current-primary-inputs-20261016.json), [joint surface](current-atlas-research-20261016.json), and [diagnostics](current-diagnostics-20261016.json) retain actual original hashes/clocks. The model's ordinary Democratic probabilities are approximatelyNY.953476, IA.297257, KS.500870 and NC.975236. These are conditional outputs of this research model, **not calibrated lower bounds, guarantees, contractual party/refund mappings or trading instructions**. Historical publication/LV gaps, current horizons outside historical support, unknown design effects and unidentified exceptional states still force `qualification.kind=research`. `currentSourceOriginalsVerified=true` reports the raw-byte checks; it does not qualify the model. No earlier forecast-cohort objective or deadline changes.

Run against the private original-source directory and exact previously normalized training file:

```text
python research/atlas_model/current-primary.test.py --sources ABS_PRIVATE_CURRENT_SOURCE_DIRECTORY
python research/atlas_model/current-primary.py --sources ABS_PRIVATE_CURRENT_SOURCE_DIRECTORY --training ABS_PRIVATE_TRAINING_JSON --out ABS_NEW_PRIVATE_OUTPUT_DIRECTORY
```

Six actual-source tests pass: exact four races/sample sizes, original publication clocks, nonrenewed receipt TTL, changed publisher bytes, retimestamped receipt rejection, missing-source rejection and no duplicated survey/forecast inputs. The exporter performs zero network requests, rejects existing output directories and writes private0600 files. The root execution owner separately controls any semantic mapping, reviewed model use and production integration.
