# Final-stage party-margin research

V2 corrects the historical outcome target before using it for current forecasting. It compares the strongest Republican candidate with the strongest non-Republican candidate, including independents. Georgia2020 uses the certified January2021 runoff, not November plurality. Louisiana2002/2014/2016 use primary FEC final-stage overrides. Certified runoff records take precedence over the initial general. Unresolved competitive party labels and ranked-choice margins are excluded explicitly. Incumbency uses first-round entrants rather than outcome-selected finalists. Strictly earlier presidential and same-seat results remain the only historical features.

The initial strict-label fit is preserved in model-summary.json and the immutable full local model-report.json:484 training rows,59/66 diagnostic directions. A separate affiliation-invariant revision permits a missing minor candidate's party only when assigning either affiliation cannot change the strongest Republican, strongest known opponent or winner. Competitive unknowns still fail. It has561 rows and82/89 diagnostic directions,11.07-point RMSE and Brier0.06991. These2020/2022/2024 cycles were already inspected; they are observed diagnostics, not untouched validation. Different coverage makes the two scores unsuitable as evidence of a trading improvement. Neither model establishes2026 calibration or profitability.

Use Node22+ from the repository root:

```powershell
node research/election-baseline-v1/download-sources.mjs
node research/election-baseline-v2/download-sources.mjs
node --test research/election-baseline-v1/model.test.mjs research/election-baseline-v2/model.test.mjs
node research/election-baseline-v2/train.mjs --verify
node research/election-baseline-v2/train.mjs --affiliation-invariant --verify
```

On a clean checkout without local reports, omit --verify to generate each named report once. The complete local report hash is in its published summary; at timestamps differ on fresh reproduction. Fitting never overwrites an existing report. Python with openpyxl runs verify-fec-workbooks.py to compare actual named GE RUNOFF columns, candidate/state/party and exact votes with override facts. Workbook originals remain local, publicly downloadable with checked source hashes. 2016's sourceLocation GENERAL wording means the final general stage; the exact verified column is GE RUNOFF ELECTION VOTES (LA).

market-screen-affiliation-invariant.json is a31-market research screen from a frozen byte boundary of the existing public order-book tape. It records ask depth and receipt age separately from assumed national-margin/incumbency scenarios. The three missing prior-seat cases remain explicit. No source clocks were renewed, no source bank was replaced and no live order or wallet budget was reset. Scenario minima are not calibrated confidence bounds. This screen has zero qualified forecasts and zero network requests.

Primary overrides: [FEC2002 Senate](https://www.fec.gov/documents/1738/FederalElections2002_Senate.pdf), [FEC2014 Senate](https://www.fec.gov/documents/1698/2014senate.pdf), [FEC2016 workbook](https://www.fec.gov/documents/1890/federalelections2016.xlsx), [FEC2020 workbook](https://www.fec.gov/documents/4228/federalelections2020.xlsx). Original historical sources and checksums are in the V1 source manifests. Original V1 results are preserved with a separate final-label-review.json correction.
