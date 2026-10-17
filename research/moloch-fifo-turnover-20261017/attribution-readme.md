# Verify actual FIFO-profit activity

[attribution.py](attribution.py) attributes **only** native quote metadata reason `reviewed_fifo_profit_turnover`. It does not include other reviewed exits, ordinary trading or whole-account PnL. It is a read-only Python standard-library tool, with no client, execution authority or model activation.

Pass an owner-pinned private capture ZIP and its independently retained SHA256. The expected packet layout is the R28/R30 first-fresh format: `packet-manifest.json` plus pinned members including `report.original.json`, `head.original.json`, `anchor-head.original.json`, `dispatch.original.ndjson` and `native.suffix.jsonl`. No extraction is needed. All original member hashes are checked. Report/head/anchor seals, the entire dispatch chain from its zero-hash genesis, and the anchored native suffix through the report's exact sequence/previous hash are verified using **original compact bytes**, without reserializing arbitrary-precision numbers.

For each matching native qid, the analyzer requires the report's metadata membership, original native cohort reservation, exact pricing basis, original pre-intent quote/spec/cohort tag and ordered attempt/ACK evidence where present. Original typed ACK receipt bytes are reconstructed in the SDK `Receipt` struct field order, preserving original nested numeric spelling, and matched to the accepted native ACK hash. Actual report fills must be owned SELLs with `counterfactual:false`, the same qid/venue binding/side/market, actual fees, and matching accepted native fill rows. Missing, conflicting, after-report or incomplete source membership is refused; it is not replaced by zero. A capture suffix too short to include a selected reservation/ACK/fill is insufficient.

Output exposes native qids, quantities, limits, actual gross proceeds, actual sell fees and net proceeds, plus frozen pricing/lot-source hashes and original clocks. Actual venue order/fill identifiers and raw receipts remain private. Native quote quantity, original prepared quantity, attempts, ACK, native terminal and actual fill totals are distinct. A filled quote's retirement does not establish complete terminal-tape closure. An open or unacknowledged zero-fill order is not a confirmed nonfill.

The reported gain is:

```text
conservative acquisition-cost upper = actual filled shares × frozen maximumFifoUnitCost
conservative gain lower bound = actual net SELL proceeds − acquisition-cost upper
```

The cost is the accepted reservation's `pricingBasis.maximumFifoUnitCost`, an upper bound on each unit in its original FIFO prefix, including native acquisition-fee upper rounding. This tool does **not** independently reconstruct exact acquisition FIFO or exact realized profit from the complete financial history. It reports `exactRealizedProfitClaim:false`, even when the lower bound is positive. Underlying original fill-tape receipt bodies and full Rust financial replay are outside this packet verifier's scope. Existing native validation remains authoritative.

Example, from the repository root:

```powershell
python -B research/moloch-fifo-turnover-20261017/attribution.py `
  "$env:LOCALAPPDATA\Temp\moloch-r28-first-live-verified-proof-20261017.zip" `
  --zip-sha256 224e5c55c55bf473a5641283fa61b97072ab618cd7e67bc4313a1bf3086efa67 `
  --out "$env:TEMP\moloch-r28-fifo-profit-attribution-negative-control-20261017.json"
```

Actual negative control: the authenticated R28 packet at **492737 / 2026-10-17T15:23:08.558Z** verifies **21,901 dispatch rows** and **256 native suffix rows** after anchor492481, then produces **zero matching quotes, attempts, ACKs and actual fills**, with zero attributed proceeds/gain. Its 382 actual fills belong to other reasons and are excluded. This is not a synthetic profit example or an execution result for the new route. No positive new-route capture has been analyzed at this handoff.

The command emits only after all validation succeeds. Preserve the output with its input ZIP pin and the emitted analyzer SHA. For R30, use its actual independently pinned ZIP/hash; never substitute the R28 hash or treat the source's existence as activity. First-fresh restoration, prospective review activation, order placement and fills remain separate facts.
