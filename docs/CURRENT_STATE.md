# Current state

## Latest historical account observation: October 3, 07:38–07:40 UTC

The parent operator's official signed-in website observations establish **zero open orders at 07:40:09 UTC**, with **RI YES 19,621**, **MA YES 2,061**, and **GA market 259 NO 10,000**. The latest history confirmation was **07:40:17**. This is historical state, not a later live-state assertion.

### Rhode Island terminal reconciliation

Order **4482453** is terminal: **12,621 RI YES filled at 0.900**, exact displayed quantity × price cost **11,358.900**; **2,379 canceled** after one cancel click at **07:38:32**. Zero orders and unchanged RI 19,621 were verified **07:38:54 UTC**. Exact execution and server-cancel times remain unknown.

The ledger appends this as **row 19**, preserving all original 18 rows byte-for-byte. The old separate 12,621-share partial-fill adjustment is retired from the derived accounting: **moving the same fill into the ledger creates no new debit or holding**. RI basis remains **17,763.900** and MA **1,999.170**. Ledger-only cash is **80,393.845**; after the preserved bot adjustment **−0.380**, known pre-GA arithmetic is still **80,393.465**.

### Georgia acquisition: filled quantity verified, exact cost unresolved

New intent **daenerys-20261003-ga259-no-buy-001** requested BUY **10,000 NO** in **Republican Georgia Senate market 259**, limit **0.895**, principal cap **8,950.000**. One click at **07:39:38** was followed by the full 10,000-share holding and zero open orders at **07:40:09**. History at **07:40:17** showed displayed average **0.895** and aggregate cost **8,945.42**. The completed order ID and underlying fills were not exposed.

The official CSV downloaded at approximately **07:41 UTC** repeats **10000 NO @ 0.895 / −8945.42**. The three-decimal average is rounded: **10,000 × 0.895 = 8,950.000 is not the observed acquisition amount**. The **4.580** difference is not labeled a fee, and no fill allocation is invented. The two-decimal 8,945.42 display also does not establish hidden execution precision.

GA is retained as a **completed holding in a separate observed-precision acquisition record**, not an open order or unsupported exact ledger aggregate. Current exact GA cost, exact total basis and exact post-GA price-product cash therefore remain **null** in the derived view. A separately labeled arithmetic bridge is **80,393.465 − displayed 8,945.42 = 71,448.045**, compared with observed UI cash **71,448.04**. That bridge is not an asserted exact cash balance. Known RI/MA basis is **19,763.070**; adding the same displayed GA amount gives a mixed-precision basis bridge **28,708.490**. Their sum cancels the provisional amount and is not evidence of its precision.

Analytical realized P&L remains **156.535**; these acquisitions created no realized sales. No exact new settlement endpoint or whole-account liquidation gain is claimed. Exact GA fills/order ID have been requested through the existing bounded read-only collector; they will be reconciled in a separate later record without rewriting this observation or double-counting GA.

Sources: [GA prepared intent](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966825237), [reported outcomes](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966858454), [exact-data request](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966866303). Immutable records: [RI terminal audit](../data/orders/daenerys_2026-10-03_ri387_buy_002_terminal_0738.json), [GA observed-precision audit](../data/orders/daenerys_2026-10-03_ga259_no_buy_001_observed_0740.json), [official CSV bytes](../data/exports/transactions_2026-10-03_0741.csv), [reconciliation](../data/snapshots/reconciliation_2026-10-03_0740.json) and [portfolio snapshot](../data/snapshots/portfolio_2026-10-03_0740.json).

The supplied CSV contains a limited set of displayed transactions; it is not asserted to be the full account history. Its exact original bytes are preserved, including its final newline state, with SHA-256 **0f67bfe1ba38e649b7553e1a7896bb9568434be7aa9bec1f165ae813c6b8c5d1**. It contains only contest transaction columns and no observed secrets. Prior historical blocks remain unchanged below.

## Latest historical account observation: October 3, 07:23–07:30 UTC

This timestamped update supersedes only the earlier portfolio observations below. The parent operator supplied the official signed-in website observations; this worker did not access the account. All previous audit/snapshot files remain immutable.

- **Order 4419984 is terminal:** 2,000 RI YES bought at **0.890**, cost **1,780.000**; 1,000 unfilled shares canceled. Fresh holdings/order observation was **07:23:20**; one cancel click at **07:23:31**; explicit zero open orders and the 2,000-share history fill verified at **07:23:58 UTC**. Exact fill and server-cancel times remain unknown
- Browser-control timeout interrupted observation approximately **07:12–07:22**. No order actions occurred in that gap. Reset recovered the same signed-in browser without new authentication
- The old order added RI holdings to **7,000**, basis **6,405.000**, displayed average **0.915**. UI cash **91,752.37** is separate from arithmetic **91,752.365**
- The parent recorded a distinct larger intent after the user's explicit **07:10** tournament-upside instruction: **daenerys-20261003-ri387-buy-002**, BUY **15,000 RI YES at limit 0.900**, maximum principal **13,500.000**. Fresh zero open orders at **07:29:19**, then one submission click at **07:29:27 UTC**
- At **07:29:56 UTC**, order **4482453** had **2,379 remaining** and RI holdings were **19,621**. History at **07:30:18** confirmed **12,621 bought at 0.900**, cost **11,358.900**. No later fill is assumed. Review/cancel remainder target: **07:40 UTC**

### Exact combined accounting and ledger boundary

The append-only ledger now has **18 executed order aggregates**: only terminal order 4419984's **2,000 executed shares** were added; all original 17 rows remain byte-for-byte unchanged. Ledger-only cash is **91,752.745**. Apply the preserved separate bot adjustment **−0.380**, then the open order's already confirmed fill cost **−11,358.900**, to obtain combined cash **80,393.465**. UI cash was **80,393.46 at 07:29:42**, a separate two-decimal observation; no hidden rounding method or fee is invented.

Order 4482453's confirmed 12,621 shares are already counted in cash and holdings through a **separate partial-fill adjustment**, although its terminal aggregate is not yet in the ledger. When a terminal aggregate is later recorded, retire this adjustment rather than deducting its cost twice. The unfilled 2,379 shares create no holding or executed cost.

RI basis is **17,763.900 for 19,621 shares**; displayed average **0.905** is not substituted for the exact basis ratio **17,763.900 / 19,621**. MA stays **2,061 shares**, basis **1,999.170**. Combined basis **19,763.070** plus cash arithmetic **80,393.465** equals **100,156.535**. Analytical realized P&L remains **156.535**; buys add no realized gain. Whole-account executable liquidation value is unknown.

If all remaining 2,379 fill at the limit, the additional cost would be **2,141.100** and RI would reach **22,000 shares / 19,905.000 principal**, about **19.9% of initial capital**. This is a risk scenario, not an observed fill. Numeric exchange ID, exact execution times, hidden precision and fees remain unexposed; the selected website context is Midterm Elections.

Sources and immutable records: [terminal source comment](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966750966), [new intent](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966767946), [terminal order audit](../data/orders/daenerys_2026-10-03_ri387_buy_001_terminal_0723.json), [partial order audit](../data/orders/daenerys_2026-10-03_ri387_buy_002_partial_0730.json), [exact reconciliation](../data/snapshots/reconciliation_2026-10-03_0730.json), and [immutable portfolio snapshot](../data/snapshots/portfolio_2026-10-03_0730.json). The historical blocks below retain their original observation times and are not later live-state claims.

## Latest historical account observation: October 3, 06:55–07:01 UTC

The following new observation supersedes the older account summary below only for its stated times. Current ownership is unchanged. The source is **the parent operator's observed official signed-in UI**, transcribed into a sanitized immutable record; this is not raw API evidence or an independently inspected screenshot.

- Before the new order, UI cash displayed **93,532.37 SUSQies** to two decimals. Exact cash-observation time within approximately 06:55–07:01 and hidden cash precision were not exposed; no post-acceptance cash display is asserted
- Holdings were **RI market 387 YES 5,000 at displayed average 0.925** and **MA market 379 YES 2,061 at 0.970**, unchanged at **07:01:51 UTC**. Historical remaining FIFO costs stay **4,625.000** and **1,999.170**, total **6,624.170**
- Account-wide zero open orders was observed at approximately **06:55:40** and at **07:01:07 UTC**
- At **07:01:51 UTC**, the account had one accepted resting order: **4419984**, intent **daenerys-20261003-ri387-buy-001**, **BUY 3,000 RI YES at limit 0.890**, remaining **3,000**, **zero fills observed**. One submission click was made approximately **07:01:16 UTC**, with no retry. Principal cap **2,670.000**; review/cancel target **07:30 UTC**. This is not an executed ledger trade
- Execution context was explicitly selected **Midterm Elections** on the official website; the numeric exchange ID was not exposed and remains null

### Separate post-ledger fill adjustment

The parent observed **12 additional displayed fill rows**, all dated **Oct 2, 2026** in the UI, across AK Republican Governor 369, VT Republican Governor 313, MA Democratic Governor 370 and FL Democratic Senate 354. The supplied reverse-chronological row order is preserved and separately replayed oldest-first. Exact execution times, fill IDs, order IDs and order grouping remain unknown: **12 fill rows are not asserted to be 12 orders**.

Exact Decimal products give purchases **21.080**, sale proceeds **20.700**, and net cash/analytical realized change **−0.380 SUSQies**, with **zero ending adjustment inventory**. Market changes are AK 369 **+0.080**, VT 313 **−0.140**, MA 370 **−0.360**, FL 354 **+0.040**. Parent-reported bot attribution is not independent order-ID attribution.

The original **17-row executed-order aggregate ledger remains byte-for-byte unchanged**. A separate immutable adjustment layer bridges its **93,532.745** cash arithmetic to **93,532.365** and analytical realized **156.915** to **156.535**. The ledger alone therefore does not represent the latest combined cash. The **93,532.37** display is **0.005 above** combined arithmetic; no hidden balance, rounding convention or fee is invented. Cash arithmetic plus unchanged position basis equals **100,156.535**, an accounting identity, not liquidation value. Future reconciliation must avoid double-counting this adjustment if underlying orders are later identified.

### Marks, audit links and limits

At approximately **06:55 UTC**, holdings were marked **6,451.68** and rounded account value was **99,984**. These are provisional valuation marks. Current whole-account executable liquidation value is unknown. The unfilled order is not deducted from cash or counted as acquired inventory; cash reservation behavior was not observed.

Sources: [pretrade observation](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966556594), [intent](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966573510), [acceptance](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966585592). Records: [immutable UI transcription](../data/snapshots/takeover_observations_2026-10-03_0655_0701.json), [exact adjustment reconciliation](../data/snapshots/bot_adjustment_reconciliation_2026-10-03_0701.json), [pending-order acceptance](../data/orders/daenerys_2026-10-03_ri387_buy_001_accepted.json), [immutable derived portfolio](../data/snapshots/portfolio_2026-10-03_0701.json), and [derived latest view](../data/portfolio/latest.json).

Validation replays the original 17 rows with Decimal and independent rational FIFO, replays all 12 additional rows with exact Decimal products and independent rational net cash, checks no oversells and zero adjustment inventory, and verifies unchanged baseline hashes. All original source audits/snapshots and the ledger are retained. The documentation below is preserved as historical context, including its older times and arithmetic. No account API, UI, order or cancellation action was performed by the record-maintenance worker.

## Preserved October 2 account history and ownership context

**Current ownership, October 3:** Daenerys / dot is the temporary sole execution owner under the human's 06:48:26 UTC instruction and 06:53:24 UTC confirmation that the bot is inactive and the PC is off. [Acknowledgment and scope](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966533944); handoff `human-20261003T064826Z-codex-to-daenerys`. Codex / Drogon must keep account writes and collectors paused, including on restart, until a new coordinated return handoff after the human says the bot is active. Old activations must not resume automatically.

The PC-off/bot-inactive state is human-reported, not a remotely verified process stop or shared-lock/lease proof. This documentation change does not control a runtime. Resting orders may still fill; fresh account reconciliation is required before any action. Prior pilot observations remain historical.

Last portfolio review: **2026-10-02 06:06:14–06:06:57 UTC**. Cash displayed **93,532.74** to two decimal places at **06:06:14**. The terminal portfolio at **06:06:57** confirmed **RI 5,000 YES**, **MA 2,061 YES**, and **zero open orders**. These are historical observations and need fresh reconciliation before any action.

## Account reconciliation

- Starting simulated cash: **100,000.00 SUSQies**
- Observed cash display: **93,532.74 SUSQies**, two decimal places; exact hidden account precision was not exposed
- Separate quantity × price ledger cash: **93,532.745 SUSQies**
- Remaining FIFO position cost: **6,624.170 SUSQies**
- Arithmetic cash plus remaining FIFO cost: **100,156.915 SUSQies**, an accounting identity rather than current liquidation value
- October 2 session realized P&L: **0.000 SUSQies**; cumulative realized P&L: **156.915 SUSQies**, under the recorded analytical FIFO aggregate-lot convention
- Open orders: **none observed at 06:06:57 UTC**
- Arithmetic reconciliation: 98,157.745 − (5,000 × 0.925) = **93,532.745**

The display is 0.005 below ledger arithmetic. Both are retained separately. The UI's hidden balance precision and rounding method are unknown; no fee, rounding adjustment, or hidden balance is invented. In schema 2, `cash_verified` and `cash_exact_verified_at_utc` remain null; explicit `cash_displayed` and `cash_arithmetic_from_verified_qty_price` fields carry the distinct values.

| Remaining market | Side | Shares | FIFO cost | Acquisition price |
|---|---|---:|---:|---:|
| Democratic Massachusetts Senate (379) | YES | 2,061 | 1,999.170 | 0.970 |
| Democratic Rhode Island Senate (387) | YES | 5,000 | 4,625.000 | 0.925 |

FL, AK and NC holdings are zero. MA and RI retain unrealized/liquidation risk; the 156.915 realized gain is not whole-account liquidation profit.

Machine-readable historical view: [`data/portfolio/latest.json`](../data/portfolio/latest.json). Immutable [06:06 snapshot](../data/snapshots/portfolio_2026-10-02_0606.json), [full-ledger replay and provenance](../data/snapshots/reconciliation_2026-10-02_0606.json), and append-only [trade ledger](../data/trade_ledger.csv). The prior [22:22 snapshot](../data/snapshots/portfolio_2026-10-01_2222.json) and all earlier snapshots are unchanged.

## Completed October 2 Rhode Island session

Exactly one new executed aggregate is recorded: **BUY 5,000 RI YES at 0.925**, cost **4,625.000 SUSQies**, submitted approximately **05:45:48 UTC** and verified in portfolio transaction history at **05:46:44 UTC**. The completed buy order ID was not exposed and remains null, with a blank ledger order-ID cell. Exact fill execution timestamps are unknown; submission and verification times are not execution times. All **16 prior ledger rows are preserved byte-for-byte**, giving **17 executed aggregates** after the append.

Covered sell **1513474**, **5,000 at 0.955**, filled **zero** shares. One initial request failed with an explicit service-unavailable message; zero open orders was verified before a second attempt was accepted. Cancellation was clicked approximately **06:05:52**, own-order removal was observed by **06:06:14**, and account-wide zero open orders was confirmed at **06:06:57 UTC**. Effective server cancellation time is unknown. The position remains held; no sell row or realized gain is recorded.

The earlier Massachusetts exit **1497695**, **2,061 YES at 0.975**, also filled **zero** shares. Cancellation was clicked **05:10:14**, with terminal zero orders verified **05:11:34 UTC**; MA inventory and basis are unchanged. The [cancellation summary](../data/orders/cancellations_2026-10-02_0606.json) distinguishes the two terminal orders and their actual observation times.

The [RI audit](../data/orders/session_2026-10-02_ri_value.json) and [MA audit](../data/orders/session_2026-10-02_ma_exit.json) are sanitized derived game-observation records. Their original source hashes and separate published-projection hashes are retained in the reconciliation record; original source bytes are unchanged. The [official UNH source](https://scholars.unh.edu/survey_center_polls/1009/) and [public Kalshi response observed at 05:41:04 UTC](../data/snapshots/ri_public_book_2026-10-02_0540.json) provide research context. Those are external references with settlement and depth limitations, not risk-free cross-venue arbitrage or booked profit.

### UI marks versus historical liquidation depth

At the final portfolio observation, RI marked value was **4,604.50** with displayed unrealized P&L **−20.51**; MA marked value was **1,975.37** with displayed unrealized P&L **−23.80**. These are UI marks, not fills. RI marked value minus recorded basis is separately **−20.500**, leaving a 0.010 discrepancy with its displayed P&L; no fee or precision rule is inferred. The holdings total was **6,579.87**; cash-display plus holdings arithmetic was **100,112.61**, while the rounded account display was **100,113**.

At **06:06:14 UTC**, RI's displayed bid was **0.900 × 40,855 shares**, sufficient depth for the 5,000-share position. This supports a historical **4,500.000 gross liquidation scenario**, or **−125.000** against the **4,625.000** basis, before fees, latency or quote movement. **No sale at 0.900 occurred.** A contemporaneous MA executable book was not captured in the final review, so whole-account liquidation value is unknown.

## Earlier historical sessions

The following session descriptions retain their original observation windows and accounting; they are not the current portfolio.

## Completed 22:19 Florida-exit session

The [immutable source audit](../data/orders/session_2026-10-01_2219.json) is preserved byte-for-byte. One limit sell was submitted at **22:21:19.621 UTC**: **7,500 Florida YES at 0.105**, immediately and completely filled. The transaction history confirmed **787.50** displayed proceeds; exact quantity × price proceeds are **787.500**. The completed order ID was not exposed and remains **null** in the audit, with the ledger order-ID cell blank. Exact execution timestamps and unexposed fill/exchange IDs remain unknown; the submission timestamp is not substituted for execution time.

Preflight found the full 7,500 shares owned and zero open orders. At **22:21:02.949**, the bid was **0.105 for 17,392 shares**, sufficient for this sale. The audit separately records the settled ticket, estimated proceeds and absence of an oversell warning before the one submission. Terminal holdings and transaction history confirmed FL quantity zero, MA 2,061 unchanged and zero open orders. No cancellation was required.

This disposal consumes the final 7,500-share Florida lot acquired at 0.120, for **900.000** FIFO cost. Session realized P&L is **787.500 − 900.000 = −112.500**; cumulative realized P&L is **269.415 − 112.500 = 156.915**. The ledger appends exactly **one executed aggregate** and preserves all 15 prior rows byte-for-byte. Its `cost_susqies` value is **−787.500**, the existing sign convention for sale proceeds. No canceled or unsubmitted intent creates a trade row.

The decision was risk reduction after a materially lower public Florida reference price. The [public-reference research bundle](../research/public_reference_2026-10-01/public_reference_report_2026-10-01.md) retains observation times, depth and settlement differences; these external prices are not guaranteed fair probabilities or arbitrage. The earlier 21:32 Florida order was canceled unfilled; this later executed sale is a distinct order and session.

## Historical sessions

The following descriptions apply only to their original observation windows, not current holdings.

## Completed 21:32 inventory-sale session

The [immutable source audit](../data/orders/session_2026-10-01_2132.json) is preserved byte-for-byte. All five submitted orders are terminal:

| Order | Outcome | Exact verified quantity × price proceeds | FIFO cost disposed | Session realized P&L |
|---|---|---:|---:|---:|
| RI YES sell 1080906 | 6,187 filled at 0.935 | 5,784.845 | 5,684.845 | 100.000 |
| AK NO sell 1082747 | 5,000 filled at 0.330 | 1,650.000 | 1,575.000 | 75.000 |
| NC NO sell 1088818 | 1,538 filled at 0.075 | 115.350 | 99.970 | 15.380 |
| FL YES sell 1084839 | 7,500 at 0.125 canceled unfilled | 0.000 | 0.000 | 0.000 |
| MA YES sell 1086473 | 2,061 at 0.975 canceled unfilled | 0.000 | 0.000 | 0.000 |
| **Session totals** | **12,725 shares sold** | **7,550.195** | **7,359.815** | **190.380** |

The 22:00:00 cancellation target is separate from actual submissions: FL at **22:00:12**, MA at **22:00:36**, once each. Fresh reload at **22:01:18** found no open orders and unchanged MA 2,061 / FL 7,500 holdings. No additional FL/MA fill was observed. The immutable audit retains the actual cancellation times even where its brief notes describe the target hour.

All RI fill-row quantities sum to 6,187, including one combined 469-share row. AK fills are 648 + 20 + 4,332; NC is 1,538. Their exact displayed tick prices and quantities determine proceeds without rounding each fill. The ledger appends exactly three executed order aggregates and excludes the two canceled unfilled orders. It preserves all old bytes, uses negative `cost_susqies` for proceeds, and retains milli-SUSQ precision. No precise execution timestamp or unexposed fill/exchange ID is fabricated.

RI disposal consumes the remaining 1,187-share 0.935 lot (1,109.845 cost) plus the 5,000-share 0.915 lot (4,575.000 cost). AK consumes its remaining 5,000-share 0.315 lot. NC consumes its 1,538-share 0.065 lot. Session FIFO gain is **100.000 + 75.000 + 15.380 = 190.380**; prior cumulative 79.035 + 190.380 = **269.415**. Cumulative per-market totals in the snapshot are replayed from all ledger sales, including the earlier RI at-cost sale. Platform weighted-average basis, tournament marks, and executable liquidation value are separate concepts.

## Earlier historical sessions

The following descriptions apply only to their original observation windows, not current holdings.

## Completed 21:02 discretionary session

The [immutable source audit](../data/orders/session_2026-10-01_2102.json) is preserved exactly as recorded. All four submitted orders are terminal:

- RI YES buy **1056531**: **5,000 at 0.915**, cost **4,575.00**; 3,104 + 1,896 shares filled
- RI YES sell **1050409**: **3,569 of 4,756 at 0.950**, proceeds **3,390.55**; **1,187** unfilled shares canceled. Fills were 1,826 + 282 + 1 + 903 + 1 + 1 + 555 shares
- AK NO sell **1052464**: **5,000 at 0.330**, proceeds **1,650.00**; 1,883 + 224 + 1 + 2,892 shares filled
- FL YES sell **1054809**: **5,000 at 0.125**, proceeds **625.00**

The cancellation target was **21:25:00 UTC**. The one actual RI cancellation was submitted at **21:25:12**. At **21:24:46**, 1,742 shares remained; **555 additional shares filled for 527.25 before cancellation finalized**, leaving 1,187 canceled. The 555 are included once in the 3,569 total. AK and FL were already filled and were not canceled. Zero open orders was verified at 21:26:00 and cash at 21:26:19. This delay and fill/cancel race are retained, not replaced with the target time.

The three superseded, unsubmitted intents (RI buys at 0.920 and 0.840; RI sell at 0.945) created no trades. The ledger appends only the four actual executed aggregates; its `cost_susqies` is positive for buy debits and negative for sale proceeds. Canceled quantities and unsubmitted intents are excluded. Exact execution timestamps, per-fill IDs and exchange IDs were not captured; they remain unknown. Submission and verification times are not execution times.

### Exact FIFO analysis

This is analytical FIFO using the previously documented acquisition lots, not the platform's displayed average basis or interim tournament marks:

| Disposal | Proceeds | FIFO cost disposed | Realized P&L |
|---|---:|---:|---:|
| RI 3,569 YES from 0.935 lot | 3,390.55 | 3,337.015 | 53.535 |
| AK 5,000 NO from original aggregate lot | 1,650.00 | 1,649.50 | 0.50 |
| FL 5,000 YES from 0.120 lot | 625.00 | 600.00 | 25.00 |
| **Total** | **5,665.55** | **5,586.515** | **79.035** |

RI remaining cost is 1,187 × 0.935 + 5,000 × 0.915 = **5,684.845**. Its exact aggregate average is **5,684.845 / 6,187**; the snapshot preserves the ratio rather than substituting a rounded average. AK retains the newer 5,000-share 0.315 lot. The disposed original AK order ID remains unknown. Prior realized P&L was 0.00, so session and cumulative realized P&L are both 79.035. The 79.04 display rounding is not used in reconciliation.

The source audit includes interim observations and the AK `proceeds_verified_by_cash_change` value 695.31. That interim value is not the terminal AK proceeds; the full 5,000-share fill and exact final cash reconcile to **1,650.00**. The original audit is not rewritten to erase intermediate evidence.

## Completed 20:36 discretionary session

[Session audit](../data/orders/session_2026-10-01_2036.json) contains exact observed order IDs, intent IDs, submission/cancellation/verification times and transaction-history fill quantities. All five order intents are terminal:

- RI YES sell **2,072 at 0.965**: fully filled, proceeds **1,999.48**, realized P&L **0.00** against the original lot. Completed sell order ID and exact execution timestamps were not exposed and remain unknown
- RI YES buy order **1023469**: **4,756 of 5,000** filled at **0.935**, cost **4,446.86**; **244** unfilled shares canceled at **20:57:06 UTC**
- AK NO buy order **1026814**: **5,000** filled at **0.315**, cost **1,575.00**. Combined with the original 5,000-share lot costing 1,649.50, remaining AK basis is **3,224.50**
- AK NO sell order **1028485**, 5,000 at 0.335: **unfilled**, canceled at **20:57:19 UTC**
- FL YES sell order **1030028**, 5,000 at 0.130: **unfilled**, canceled at **20:57:39 UTC**

The cancellation target was 20:57 UTC; actual cancellation submission times and the later zero-open-orders observation above are authoritative. No additional fill/cancel race was observed. Each cancellation was submitted once; a transient Florida UI wait did not trigger a duplicate submission.

The ledger adds only three actual trade aggregates, not canceled unfilled quotes. Its existing `cost_susqies` column is signed cash cost: positive for purchase debits, negative for sale proceeds. This permits initial cash minus cumulative signed cost to reconstruct cash. It is not a realized-P&L field. Exact fill timestamps remain unknown; submission and verification times are not substitutes. The source audit preserves the 100+1,972 RI sale fills and 1+3,907+2+1,090 AK purchase fills without inventing execution IDs. Rounded AK fill displays sum to 1,575.00, while exact per-fill products can carry half-cent precision; aggregate accounting uses the exact 5,000 × 0.315 cost.

## Important limitations

The portfolio page uses tournament valuation prices whose formula remains unverified. Its marks differed sharply from detail-page prices. Neither is a guaranteed full-position exit price. Current displayed gains/losses are not realized P&L. No fee or hidden-precision rule is inferred. Realized gain excludes all remaining position risk.

See the [execution and valuation review](../research/execution_valuation_review_2026-10-01.md) for historical depth and observation windows. Quotes were not captured simultaneously and are stale for execution.

Under standard 0-or-1 settlement and no further trades, fees, refunds or sponsor interventions, ledger-arithmetic endpoints for the two remaining MA/RI positions are **93,532.745** if both lose and **100,593.745 SUSQies** if both win. These use the separate arithmetic balance, not an asserted hidden UI balance; they are scenarios, not forecasts or executable valuations.

## Historical operating mode (October 2)

Codex is the sole execution owner under the human's October 2 reassignment. Daenerys remains advisory; the former account worker was confirmed stopped in issue comment 5947535773. The historical portfolio observations above are preserved separately from current bot-attributed activity.

The same small pilot run, `sig-pilot-20261002-0824`, was renewed at **2026-10-02 14:18:24.002 UTC**, with new entries authorized until **20:18:24.002 UTC**. The original authorization is archived. Cumulative accounting was retained: **4.360 SUSQ** of the unchanged 100 SUSQ cumulative purchase ceiling has been consumed. Limits remain 25 SUSQ per order, 100 SUSQ aggregate attributed cost, three markets, 1% displayed depth and zero added fees. The ownership file is a coordination record, not an execution lock.

The collector restarted with the authorization-window correction in commit 8d5c4d4a7a52c82f0702e57ca62398d145a2ce5a. Fresh official account, market and book bootstrap completed at **14:19:11.843 UTC**. At 14:19:54, six attributed orders were reconciled across two completed roundtrips, with no open bot intent or bot-owned inventory and **+0.035 SUSQ realized P&L**. That tiny sample does not establish profitability or justify scaling; it excludes the older unrelated holdings described above. Current runtime observations are displayed in the local dashboard.

BUY authorization is rechecked at preparation and both queued dispatch fences. Owned exits, cancellation and reconciliation remain available outside the entry window; any expired recovery reuses the previously attempted exact key and payload.

## Historical bot work queue (paused)

The following prior work queue is preserved for a future coordinated return; it does not authorize restarting workers, collectors or account writes during Daenerys's temporary ownership.

1. Keep one collector and all existing workers healthy, preserving frozen banks, source journals and cumulative risk limits.
2. Compare the fixed dip/rebound wallets on subsequent live observations; recent quiet price paths have produced no qualifying signals.
3. Test price-protected entry execution prospectively under unchanged discount gates. Two observed official entries caught short-lived ask concessions; this motivates a test, not a profitability claim. A quote touch or depth disappearance is insufficient proof of a resting fill.
4. Monitor actual completed-trade economics and fresh account reconciliation before any scaling or further pilot-window renewal.
