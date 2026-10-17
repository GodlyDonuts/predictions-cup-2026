# Historical FIFO turnover census — October 17

**None of the ten held reviewed scopes has a FIFO prefix earning at least one tick net at the supplied historical top bid.** NC966 is flat. Five scopes have potentially competitive passive profit limits; their gains below require future actual fills. This analysis establishes no immediately locked profit, current executable opportunity or execution authority.

The original R28 report is sequence **492737**, at **2026-10-17T15:23:08.558Z**, captured **15:23:09.433188Z**, with 382 actual fill rows. The later quotation fixture was supplied by root with the time **16:14 UTC**, at minute precision. Its original receipt bytes, exact receipt clocks and receipt hashes were not provided. The report and quotes are therefore asynchronous historical inputs; intervening position, reservation and market changes are unobserved here.

All prices below are on the **held side**. NO bid/depth equals `1 − YES ask`/YES ask depth; NO ask/depth equals `1 − YES bid`/YES bid depth. Ask depth is competing supply, never capacity available to execute a SELL.

| Exchange / held side | Held shares | Historical bid × depth | Historical ask × depth | One-tick net floor for first min(held, 10,000) | Existing reviewed value tick floor | Maximum competitive profitable FIFO prefix / limit | Net gain **if fully filled** |
|---|---:|---:|---:|---:|---:|---:|---:|
| 1042 YES | 430 | .915 × 7,505 | .920 × 2,868 | .925 | .945 | 0 | — |
| 1051 NO | 11,602 | .855 × 7,980 | .865 × 45,687 | .870 | .900 | 0 | — |
| 1067 YES | 31,217 | .310 × 4,355 | .315 × 1,910 | .320 | .495 | 0 | — |
| 1068 YES | 7,061 | .950 × 4,571 | .955 × 34,395 | .975 | .985 | 0 | — |
| 851 NO | 10,773 | .875 × 4,892 | .880 × 8,493 | .880 | .890 | 10,000 / .880 | 72.160 |
| 865 YES | 11,521 | .860 × 4,032 | .875 × 5,738 | .880 | .930 | 3,278 / .875 | 19.010 |
| 872 YES | 6,521 | .460 × 1,740 | .470 × 5,082 | .465 | .550 | 6,521 / .465 | 32.605 |
| 885 YES | 14,166 | .705 × 1 | .710 × 18,772 | .710 | .780 | 10,000 / .710 | 50.000 |
| 966 YES | 0 | .905 × 773 | .910 × 164 | — | .895 | 0 | — |
| 977 NO | 10,675 | .925 × 1,304 | .935 × 4,254 | .950 | .970 | 0 | — |
| 993 YES | 14,100 | .715 × 1,873 | .720 × 8,325 | .720 | .760 | 10,000 / .720 | 90.960 |

“Competitive” here means the computed minimum SELL limit is no higher than the supplied own-side ask. It is a price comparison, without a queue, arrival-rate or passive-fill probability estimate. The quantities are capped at the original native **10,000 shares per order**; they are not cumulative cohort limits. All eleven scopes had zero active native quotes in this report, but that is not current physical liability clearance.

The distinction between a FIFO prefix and average cost matters. MA starts with 2,061 shares at .970, before 5,000 at .925. Selling the cheap lot alone is unavailable: the one-tick floor for the 7,061-share prefix is .975. ME can offer its first 3,278 at .875, but the next 251 shares cost .875 and require at least .880. AK's first 10,000 have maximum unit principal .315 and require .320; applying the worst cost of **all** 31,217 shares would unnecessarily raise that smaller prefix's floor. OR begins with 380 at .935, so its .925 historical NO bid cannot produce the required gain despite cheaper later lots.

The five conditional figures use these exact FIFO principals:

| Exchange | Quantity | FIFO principal | Conditional proceeds at limit | Conditional net gain |
|---|---:|---:|---:|---:|
| 851 | 10,000 | 8,727.840 | 8,800.000 | 72.160 |
| 865 | 3,278 | 2,849.240 | 2,868.250 | 19.010 |
| 872 | 6,521 | 2,999.660 | 3,032.265 | 32.605 |
| 885 | 10,000 | 7,050.000 | 7,100.000 | 50.000 |
| 993 | 10,000 | 7,109.040 | 7,200.000 | 90.960 |

Original manifest/control fees are **0 bps plus .000000/share**, and recorded native fees are zero. The analyzer implements native integer-micro-unit fee rounding rather than assuming a waiver. Unknown fees incurred before inherited adoption are excluded; these conditional figures are relative to the recorded FIFO principal, not independently established lifetime net profit. Lot order is preserved exactly, without sorting execution timestamps or selecting the cheapest holdings.

The comparator is source candidate `111abff39d9391d7e1840fc2ae6d7e785828c06c`, specifically `fundamental_policy::structure`, `passive_value_limit`, and native `fee`. Existing value-exit floors use the review's objective-specific scenario payout, with entry margin added back exactly once, and official tick/fee rounding. These are explicit uncalibrated owner-reviewed assumptions, not validated fair prices. A lower FIFO-profit limit changes the exit objective; this census neither approves that change nor demonstrates that realizing an accounting gain beats holding the position. Expected value, correlated exposure, holding time, prize utility and capital shadow price remain unresolved. No ranking override is proposed from this table.

## Reproduce from retained originals

Python 3.10+ standard library only; no network or account client. The private ZIP remains outside Git. From the repository root:

```powershell
python -B research/moloch-fifo-turnover-20261017/analyze.py `
  "$env:LOCALAPPDATA\Temp\moloch-r28-first-live-verified-proof-20261017.zip" `
  --out "$env:TEMP\moloch-fifo-turnover-regenerated.json"
```

The command verifies the exact ZIP SHA and every member's raw bytes/hash, then reads original FIFO positions and the last retained review-generation values. It emits only aggregates, source hashes and clocks. It does not replay the Rust engine, authenticate underlying venue receipts, renew reviews, choose current orders or manufacture fills. All original review source hashes, declared publication/observation/expiry clocks and typed review hashes are retained in [aggregate.json](aggregate.json); those declarations are not an independent publisher-clock audit.

- Private ZIP SHA256: `224e5c55c55bf473a5641283fa61b97072ab618cd7e67bc4313a1bf3086efa67`.
- Original report raw SHA256: `9001d57bed3eff63a2cdc0d5da3b4ead2a64139f942f7ca460ecd99bb5454337`.
- Original fundamental sidecar raw SHA256: `e294520aff0a0b2571e0b7f57a98aff0f7c7a4026b70bdcf15d7f6176a4650f2`.
- Historical supplied-book fixture SHA256: `d691881a334237ad716963426af2ca84dd221625c0e9ea26b42ba71e2f9ea41b`.

Actual validation: ran the analyzer on that retained private ZIP and read back all eleven rows, including zero profitable bid-prefix quantity and flat NC. A second regeneration was compared byte-for-byte. Runtime behavior, source clocks, controls and execution guards were unchanged.
