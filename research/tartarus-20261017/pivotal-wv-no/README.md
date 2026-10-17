# West Virginia NO: a pivotal source question

This source-only audit identifies a **hold-versus-confirmed-sale sensitivity**, not an instruction to sell. The frozen account holds 16,438 West Virginia Republican-party NO shares. A genuine, older book implies a .070 NO bid; the newly captured candidate forecast implies a very different ordinary outcome under an explicit candidate-to-party assumption. The exception assumption and whole-account joint state can reverse the decision. There is no current executable quote, calibrated probability, rank advantage, new review or execution authority here.

## What the originals establish

The retained SIG node is market297 / exchange986 / contract298: West Virginia U.S. Senate, General, November3,2026, Party Winner, Republican Party. It schedules settlement November4 at17:00UTC but supplies no detailed party-affiliation, replacement, fusion, write-in, tie, refund or resolver-selection clauses. Its original receipt is **October16 02:27:14.132UTC**; this audit does not renew it.

The [official Boone County general-election sample ballot](https://apps.sos.wv.gov/elections/2026GeneralSampleBallots/SAMPLE%20-%20WV%20Boone%20261103%20General%20-%2014inch.pdf), captured **October17 19:26:05.168UTC**, names Shelley Moore Capito REP, Rachel Fetty Anderson DEM and S.Marshall Wilson CST for U.S.Senator, plus a write-in line. The page was rendered and visually checked. This is a current general ballot, not a primary candidate list; it does not enumerate certified write-ins or prove that no later correction exists. Raw ballot bytes remain private.

The [Call the Map page](https://www.callthemap.com/senate/west-virginia), captured **October17 19:26:04.206UTC**, displays Capito's win point .996 and Anderson's .004. It says no current nominee-matched poll average is available. Wilson is not a named win-probability row; the separate 3.4% Other vote-share allocation is not a win probability. This is one publisher's uncalibrated point, not an independently validated polling estimate. Its `dateModified=2026-10-17T16:54:37.000Z` is publisher metadata, not proof of this edition's first availability. The older pinned October16 edition also displayed .996; neither capture is backdated.

The [West Virginia vacancy statute](https://code.wvlegislature.gov/3-5-19/) contains conditional party-nomination replacement procedures and deadlines. It does not establish the venue's resolution treatment. The [official SIG settlement guide](https://sig.thesuper.market/docs/settlement-and-payouts) describes cancellation/N/A principal refunds and settlement remaining pending until valid source results; its scheduled date does not guarantee immediately spendable cash.

## The falsifiable decision boundary

Let C mean Capito wins and R mean the contract resolves Republican YES. **Only in an ordinary valid binary-settlement regime where C implies R**, applying the declared model point gives `q_NO <= 1 - .996 = .004`. Other Republican winners can make this inequality stricter; Anderson's chance alone is not an exhaustive NO probability. A resolver that uses a different affiliation date or winner predicate, replacement/fusion treatment, cancellation/refund or an unmatched election would invalidate this simple correspondence. The node and ballot do not resolve every such case.

For sensitivity only, assign an **unestimated** exceptional mixture mass `e` and give every exceptional state the maximally favorable NO payout1. Then:

```text
declared NO payout upper scenario u(e) = (1-e)*.004 + e = .004 + .996*e
old gross sale reference b = .070; historical fee policy = 0
u(e) = b at e = (.070-.004)/.996 = 11/166 = .066265060240963855...
```

This is algebra under stated assumptions, not a measured exception probability or statistical upper bound. It also gives refunds a favorable payout1 even though the remaining native principal is only .115/share. Actual refunds must use original remaining lots; they cannot be silently priced as binary wins.

| Assumed favorable exception mass | Scenario payout/share | Scenario payout,16,438 shares |
|---|---:|---:|
| 0 | .004000 | 65.752000 |
| .03 | .033880 | 556.919440 |
| .10 | .103600 | 1,702.976800 |

The old .070 bid has 24,671 shares of displayed complementary depth, giving a historical gross sale reference of **1,150.660000** for the frozen holding. Its source clock is **October16 22:26:43.7196265UTC**, received22:26:43.863UTC. It precedes this forecast capture: the comparison is asynchronous and is not a trading backtest. No fill is inferred. The full-position amount is a comparison aggregate; original per-order quantity limits and all physical checks still bind. The original 1,890.370000 principal is sunk for continuation choice; realizing a loss is neither sufficient reason to sell nor a reason to hold.

## What would change a decision

A governing original resolver clause contradicting `C => R` would remove the .004 ordinary scenario restriction. Otherwise, a reviewed joint/exception model and a genuine current bid/depth could make this a concrete sale/hold alternative in the full-account frontier. The available documents **do not settle the resolver gap**. Even when monetary expectation favors sale in one scenario, retaining this tail may improve a declared first-place state; joint dependence, rival thresholds and a feasible funded replacement must be compared. No such ranking has been established.

The minimum next owner input is a genuine current target book and exact governing resolver evidence. Any replacement must separately qualify at its actual depth and source deadline, and use only cash confirmed after sale fills; neither a sale ACK nor historical proceeds fund it. This is a source priority for mandate E/F, not a source renewal or allocation approval.

## Pins and reproduction

[`evidence.json`](evidence.json) records all original hashes, UTC request/receipt/source clocks, the frozen report boundary and exact sensitivity quantities. Account ZIP `9d32ccb0986eb9a1430a396df03bd2c8df7abce6475f302a0344c16afa3aa5e2` contains the original report at native523602 / **October17 18:13:47.933UTC**; report rawSHA `3f95693ad82bba9e03863e12bffa2402d4c0b68a6e7afecb0388a76aa505f178`. `bank_head` is the exact four-field original stopped head, hash `3f5df5f6789ae7fad1851c0984ef0f9a01a8033120e86bb04beaeaf75c08b73d`; the different report outer seal `914186a4c2f24a3057355f7c5668506e3c21104e0a6cc307e776418b263f2ba4` is explicitly `reportSealSha256`, with report time separate. The existing pinned raw-token verifier checked both original seals and packet-member pins, plus equal sequence/manifest/native-tail references, without reserializing report numbers or replaying the financial prefix. Only the selected aggregate is published, with no venue order IDs, contacts or raw private positions.

Private public-source capture: `%LOCALAPPDATA%/Temp/moloch-tartarus-pivotal-wv-no-20261017-8c1gfiot`, manifestSHA `a76fa7acc9e54fe1c83dcdb858305eee839a4c3080d453a2a170c598711819ba`. Original sole-worker node/book references remain under `%LOCALAPPDATA%/Temp/moloch-paper7-original-market-receipts-20261016`; no SIG request was made. Four bounded ordinary public HTTPS captures succeeded; raw bodies and headers were reread and hash checked. Python's initial DNS failure is not represented as a capture. No financial replay, model fitting, source expiry extension, service or account action occurred.

To reproduce the arithmetic with the standard library: `Decimal(16438) * ((1-Decimal(e))*Decimal('.004')+Decimal(e))` for `e` equal to0,.03,.10; compare with `Decimal(16438)*Decimal('.070')`, and compute `(Decimal('.070')-Decimal('.004'))/Decimal('.996')`. Hash raw files with `hashlib.sha256(path.read_bytes())`; parse JSON prices with `parse_float=Decimal`, never a binary float. Frozen evidence files are not outputs to overwrite.
