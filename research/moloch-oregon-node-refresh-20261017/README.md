# Retained Oregon node: offline provenance only

**Root candidate status, October17 12:07 UTC:** a new immutable OR review and
settings packet passes actual Rust review/settings validation and independent
[20-file candidate verification](candidate-root-verification.json). It remains
inactive. Only the genuine node reference and its actual observation change;
the continuous reviewed ceiling remains .955150 (the venue execution path
floors separately to .955), and probability, margin, caps, holding objective and
original CTM/ballot clocks remain unchanged. All26 historical approval hashes
and the other ten review references are retained. No worker rebuild or restart.
Runtime installation still requires fresh same-worker account reconciliation
and no pending/open Oregon order, after R26 fresh-operation verification.

Task [168](https://github.com/GodlyDonuts/predictions-cup-2026/issues/168).
The verifier ran successfully against the existing private originals. Its
[aggregate result](node-provenance.json) contains no raw contract/creator body.
The provenance verifier itself performs no request, candidate review,
authority/settings change, hotreload or activation. The separately verified
root candidate above is not a new forecast, higher allocation or profit claim.

The existing sole worker really received the newer official market288 / contract289
node at **October16 19:17:11.799 UTC**, after its request at19:17:11.545.
The original extraction manifest pins907 bytes, raw SHA
`e6d1b884d628881ffda0fdf86a8ac3ec3b8c974ec60ba5635c4641a7deff664f`.
The separately computed typed Receipt digest is
`906ba379ab9b8d8dd1497e11fdc32bbd3290cf6e77f6a59816411310fc6ed664`,
matching its original receipt-store filename. **These are different hashes.**
Receipt serialization retains its Rust field order
`endpoint, requestStartedAt, receivedAt, status, data`; only the nested JSON
Value object keys are sorted. Sorting the entire Receipt changes its digest.

The newer and old roots are exactly equal, including settlement November4
17:00UTC. This supports a genuinely later observation of unchanged metadata;
it proves no new publisher update, independent resolver interpretation or
current executable book. Root SHA remains
`ebe719c4fc2796abfc66e2669db45e84abf9c1c7310e00dc298f05f5c2c60d15`.
The whole active packet and63-row original sidecar chain bind the original
fourth generation and eleven reviews. The original review hash is a retained
declared Rust hash; this verifier does not pretend sorted Python JSON recomputes
typed ReviewInput serialization or independently approve that review.

Any future owner-approved review must preserve CTM observation
**Oct16 18:34:55.251 UTC** and ballot observation **18:34:59.736 UTC**, their
original bytes/publication fields and24-hour TTLs. Replacing only the metadata
reference could therefore leave CTM as the binding expiry at **Oct17
18:34:55.251 UTC**. It does not grant that extension. Preserve original holding
objective, party, assumptions, caps and attribution; a separately approved typed
review/source validation and genuine generation are still required. Keep old
artifacts and historical approval hashes. Do not edit a held source or authority.

Reproduce from this repository (PowerShell; private inputs are read only):

```powershell
python -B research/moloch-oregon-node-refresh-20261017/verify_retained_node.py `
  --retained-directory "$env:LOCALAPPDATA/Temp/moloch-paper7-original-market-receipts-20261016" `
  --active-packet "$env:LOCALAPPDATA/Temp/moloch-profit-exit-timing-live-proof-20261017-v2.zip" `
  --old-node "$env:LOCALAPPDATA/Temp/moloch-oregon-terminal-candidate-20261016/official-contract-node.json"
```

Optional `--output NEW_PATH` uses exclusive creation and never overwrites.
The verifier checks pinned original hashes, manifest identity and acquisition
clocks, exact endpoint/status/tournament/market/contract, original sidecar chain,
old review/source binding and unchanged root/settlement. It has no network code.
