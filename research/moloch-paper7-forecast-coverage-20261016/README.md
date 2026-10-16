# Published forecast coverage beyond the six models

This finite screen finds **77 exact structured Senate/governor Party Winner contracts across 49 races**, including **43 races beyond the six active at assignment**. It captures all 35 Senate and 36 governor race pages published by Call the Map. Under an explicitly illustrative 3% exception allowance, zero exceptional payout and 1-cent margin, 46 contract/side rows across 33 additional races clear their **historical reference** ask. These are leads for independent review, not executable quotes, calibrated edges, allocation approvals or fills.

The source base is `817e0602bd6c7642c6b574a92d6d3c1081385475`. Only this research directory changes. No account request, SSH session, service, native ledger, control, runtime or model activation was used or changed.

## Three concrete leads

Prioritize the direct Republican YES contract when reviewing the same economic exposure: another party's NO may include additional independent winners and exceptions, so the two are not assumed identical. Books below come from separate original receipts and are **not synchronized**.

| Additional race and ordinary model candidate | SIG market / exchange / contract; side | Published candidate point | Dated ask × top depth | Original book source UTC | Research ceiling |
|---|---|---:|---:|---|---:|
| Nevada governor, Joe Lombardo (R) | 183 / 872 / 184; YES | .783 | .460 × 4,353 | Oct 16 18:24:57.0360082 | .745 |
| Minnesota Senate, Michele Tafoya (R) | 361 / 1050 / 362; YES | .332 | .140 × 8,372 | Oct 16 18:31:08.6480551 | .310 |
| Vermont governor, Phil Scott (R) | 313 / 1002 / 314; YES | .918 | .735 × 6,628 | Oct 16 22:06:29.3269212 | .880 |

These points come from the visible candidate-win tables of the original [Nevada](https://www.callthemap.com/governors/nevada), [Minnesota](https://www.callthemap.com/senate/minnesota) and [Vermont](https://www.callthemap.com/governors/vermont) public pages, captured at 22:51:15.121401, 22:51:24.545441 and 22:51:18.547018 UTC respectively. All three original pages publish `dateModified=2026-10-16T17:57:26.000Z`. A later web-tool representation of Nevada returned the older Oct 15 .785 reading; that representation is not substituted for the pinned original Oct 16 .783 body.

The ceiling is `floor_tick(.97 × selected ordinary candidate model point − .01)`, tick .005, fees assumed zero for this research arithmetic. The .03 allowance is an owner-style **assumption**, not a measured exception rate or uncertainty bound. Native funding, aggregate held exposure, actual fees, quantity, price, depth and current source checks remain separate. Top depth is not permission to spend a whole budget or a prediction that an order will fill. For example, the old Vermont top depth costs 4,871.58, while the old Minnesota top depth costs 1,172.08; neither amount is a proposed order.

The smallest useful next step is an independent official general-ballot check for these three named candidates and party labels, followed by root review of original source bytes and current sole-worker facts. Public official locators are [Nevada's election information](https://www.nvsos.gov/sos/elections/election-information/2026-election-information), [Minnesota's general candidate finder](https://candidates.sos.mn.gov/) and [Vermont's candidate list](https://sos.vermont.gov/elections/election-info-resources/candidates). Those ballot checks were **not completed by this screen**. The exact retained SIG roots prove the party predicates, not that a candidate model is contract-equivalent under every resolver exception.

## Mapping and uncertainty

`coverage.json` contains every race, visible named candidate, exact displayed win point or explicitly censored interval, original publication/observation clocks, source hashes, matching contract predicates and dated reference books. Matching requires structured `contract_details`: election date Nov 3, 2026, General stage, correct state, exact office and Party Winner resolution. `root.node_id` must equal `root.contract_id`; no title or `root.id` fallback is used. Exact tournament contexts, original endpoints, body hashes, book scope, original version clocks, tick prices and two-sided depth are checked. All 312 exported original receipt files are hash/byte checked, including the non-target files. All 77 matched contracts have usable dated original book receipts.

Named candidates contribute only their displayed win points. A named candidate's ordinary victory is a conditional proxy for the matching party YES, or another party NO, **only if the party correspondence holds at resolution**. Multiple same-party candidates are summed without turning the opponent's probability into a party complement. Independent winners are explicit: for example, Rhode Island governor has separate Democratic, Republican and independent model points. Unpublished aggregate Other vote shares are not probabilities. Published `<0.1%` / `>99.9%` values remain censored; Jack Reed's Rhode Island Senate `>99.9%` is not silently converted into an exact .999 point or a new review.

The nodes do not include full resolver prose. Fusion/cross-endorsement, party affiliation at the resolver's relevant time, replacement, runoff/RCV, cancellation, N/A/refund and discretionary rule updates remain review risks. Ordinary candidate probabilities do not estimate those risks. A refund can depend on actual principal rather than a fixed per-share payout. No deterministic floor, guaranteed two-party complement or new semantic certificate is granted.

The publisher's [methodology](https://www.callthemap.com/methodology) describes a shared national/regional/state simulation, historical validation over four election cycles and declared candidate-mechanics assumptions. It says market prices do not feed its forecast. That is source-described independence from venue prices, **not our independent replication or calibration**. More rows from one model do not provide independent models or independent portfolio risks. This screen neither averages correlated forecasts nor translates terminal probabilities into an earlier liquidation value. Scheduled SIG settlement is retained separately from election day.

Twenty-two CTM races have no matching original node in the supplied bundle; this is a retained-source gap, not evidence that SIG lacks those markets. Every race also explicitly retains pending official candidate/party and exception review gaps. Existing active races are marked and excluded from the additional-lead list: AK/DE/MA/NC/OR Senate and Maine governor. A complementary party contract in one of those races is not counted as new diversification.

## Original evidence and reproduction

Private source directories, preserved without raw candidate personal files in Git:

- Public originals: `%LOCALAPPDATA%/Temp/moloch-paper7-forecast-coverage-20261016/raw-r1`. Seventy-four original HTTP 200 bodies and header files, requested/received **22:51:07.357502–22:51:29.694084 UTC**. Source manifest SHA256 `69aa0a70d59c72e68a5992721cea03951de746590d214a81ea56b797ef22e90c`. Header HTTP statuses and header hashes were independently checked for all 74. All 71 race JSON-LD dates are Oct 16 17:57:26 UTC; HTTP cache/server dates are preserved separately, never used to renew a forecast.
- Sole-worker retained originals: `%LOCALAPPDATA%/Temp/moloch-paper7-original-market-receipts-20261016`. Original export manifest SHA256 `9579856dcdfc084b75d6f6a4defcec8dab20ed817c97ae056a9a956aba500e17`, extracted **22:50:53.999755 UTC**, 135 node plus 177 book receipts. Parent supplied ZIP SHA256 `9b722a68ae34889c3f1131b97ef59d4e542b081c149721a07cd49cadf31845df`; this analyzer independently checks extracted raw files against the manifest, rather than claiming another ZIP verification.

The old book ages at export and exact request/receipt clocks are in the artifact. No full provider continuity, new account truth, current own-order exclusion or economic replay is claimed. The forecasts were first observed by this capture after the retained books: comparing them is an asynchronous nomination, **not a causal trading backtest**.

```powershell
python -B -m unittest discover -s research/moloch-paper7-forecast-coverage-20261016 -p test_analyze.py -v
python -B research/moloch-paper7-forecast-coverage-20261016/analyze.py "$env:LOCALAPPDATA/Temp/moloch-paper7-forecast-coverage-20261016/raw-r1" "$env:LOCALAPPDATA/Temp/moloch-paper7-original-market-receipts-20261016" > coverage-reproduced.json
```

`analyze.py` uses only the standard library and makes no network requests. `capture_public.py NEW_PRIVATE_DIRECTORY` is the separate bounded public acquisition path: exactly two index pages, their 71 linked public race pages and methodology, at most two requests in flight, maximum 4 MB/body. It uses ordinary HTTPS public pages, no internal API or account client. Existing captures are never overwritten. A future acquisition is a new observation and will have different hashes; it does not rewrite this result.

Checks: **11/11 synthetic regressions pass**, covering source substitution, exact scope/root identities, candidate win versus vote share, censored/unpublished probabilities, independent/same-party outcomes, original clocks, NO complement/depth, malformed books and exact tick arithmetic. The full pinned 74-body/312-receipt offline derivation completed successfully. No profitability or prospective fill claim follows from those checks.
