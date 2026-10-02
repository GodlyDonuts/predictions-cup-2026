# Exact Cup identities and conditional cross-venue screen · 2026-10-02

Historical research bundle: **237 exact Cup market-ID/title identities**, 49 reverified in the UI on October 2, support **226 conditional office/state/district/party matches** against the saved public reference. There are 11 unmatched markets, no multiple-reference ambiguities, 451 rankable side comparisons, one suppressed zero-size comparison, and 99 positive stale quote gaps. These counts do not establish settlement equivalence or an executable opportunity.

## Start here

1. Read `report.md` with its original **05:36:19.831492 UTC** analysis time in mind. Its recommended UI checks and price gaps are historical, not current instructions.
2. Read the three later UI audits: `ri_book_check_2026-10-02.json`, `secondary_book_checks_2026-10-02.json`, and `alaska_complement_check_2026-10-02.json`. Every book has its own observation clock; the audits are not synchronized snapshots.
3. Run `python verify_bundle.py` for offline hashes, lossless decompression, identities, counts and exact-decimal gap checks.
4. Run `python reproduce_screen.py` to reproduce all ten original screen outputs from pinned local inputs. In a repository checkout, the sibling `../public_reference_2026-10-02/` directory supplies the public reference. Otherwise pass `--reference-dir PATH` to the downloaded pinned public-reference bundle. No network requests occur.

## Historical account and execution boundary

The RI audit includes the authorized simulated-account alias, displayed balance and no-position observations from its original read-only inspection. **Those statements predate later RI execution and must not be used as current account, holding or open-order state.** Fields such as `desired_action`, `price_ceiling`, proposed UI checks, and numerical rankings record the historical research question only. This publication neither creates an order nor changes the trade ledger, portfolio, execution ownership, application code, collector, or current-state documents.

The local analysis made no new Cup/public-reference requests. Packaging, verification and reproduction also make no market requests. Source verification used read-only GitHub reads. The original source scripts are included as evidence; use the safe temporary-directory reproduction wrapper rather than running them directly against their original hard-coded output paths.

## Published files

Large JSON files are losslessly compressed with gzip level 9, mtime 0 and an empty filename field:

- `cup_ui_identity_map_2026-10-02.json.gz`: 237 exact identities; 49 live-reverified and 188 still based on the archived official UI catalog
- `public_market_inventory_2026-10-01_184851.json.gz`: original 237-market official UI catalog; its hash matches the identity map's provenance field
- `cup_bulk_leads.json.gz`: exact saved 237-exchange bulk source bytes from the pinned repository export
- `join_records.json.gz`: all 237 identity joins and unresolved semantic requirements
- `conditional_crossvenue_leads.json.gz`: 451 rankable conditional comparisons
- `unmatched_and_ambiguous.json.gz`: unmatched/ambiguous classifications and full-identity blocks
- `cup_separate_party_pair_diagnostics.json.gz`: 117 separate-party price diagnostics, never assumed complementary

Uncompressed files:

- `report.md`, `summary.json`, `top10_candidates.json`, `ui_check_shortlist.json`
- `suppressed_side_observations.json`: the one unranked zero-public-size observation from the 452-row all-side dataset
- The three historical UI book audits named above
- `build_screen.py`, `write_report.py`: exact original offline source scripts
- `verify_bundle.py`, `reproduce_screen.py`: portable offline verification/reproduction helpers
- `source_manifest.json`: exact original and compressed sizes/SHA-256, source mappings, omitted files and commit pins
- `publication_validation.json`: packaging validation results
- `bundle_manifest.json`: SHA-256 and Git blob SHA-1 for every published file except itself

Read compressed JSON directly:

```python
import gzip, json
with gzip.open("conditional_crossvenue_leads.json.gz", "rt", encoding="utf-8") as f:
    leads = json.load(f)
assert len(leads) == 451
```

Or decompress with `gzip -dc FILE.json.gz > FILE.json`. On PowerShell, use Python binary I/O to preserve exact bytes. Both compressed and uncompressed SHA-256 values for every gzip are recorded in `source_manifest.json` and checked by the verifier.

## Pinned source inputs

- [Cup bulk source at 3175b7c](https://github.com/GodlyDonuts/predictions-cup-2026/blob/3175b7c12c12626f15c925eaf1d5a6ba27a215aa/data/evidence/breadth-20261002-0525/leads.json): original SHA-256 `4054acf1eacca0db6f5b641f7b205a0ed6366da413fb1c7fce13d1a7e732d9ec`
- [Public reference at c0782e9](https://github.com/GodlyDonuts/predictions-cup-2026/tree/c0782e9a6214ca9009bd70924737df64d00f4330/research/public_reference_2026-10-02): compressed reference SHA-256 `b4c4c2cf6cfc1de7b5e505356422d741de49a81aee230f4880fda1ebe195b034`; uncompressed reference SHA-256 `bfa0d2095071986841a2ef302385a7565f0574c4d13a2b909edb6eec6bb1cb1b`

These exact pins are retained even if main advances. The public reference's omitted-raw-evidence limitations still apply. Local original paths retained inside source records are provenance strings; the portable wrapper maps the relevant data to a temporary directory.

## Reproducibility and omissions

The duplicate `conditional_crossvenue_leads.csv` and mostly duplicated `all_joined_side_observations.json` are omitted. Their original hashes are retained; the ranked JSON plus the one suppressed row preserves all 452 side observations. The offline wrapper reconstructs both omitted files and verifies their original byte hashes, along with eight other generated outputs. It freezes the original analysis time and preserves historical provenance strings while adapting only local input/output paths.

Reproduction verifies deterministic transformations of saved evidence. It does not independently authenticate original UI/API observations, prove full contract equivalence, or make the old quotes current. The original `report.md` mentions uncompressed filenames and duplicate files from the source workspace; use this README and `source_manifest.json` for the publication layout.

## Interpretation limits

- All Cup bulk quantities and both venues' source-quote ages remain unknown; receipt time is not quote creation time
- Cup bulk receipts are mixed and older than the public reference and later UI checks
- Cup year uses official tournament context; title joins alone leave stage, regular/special status and exact settlement predicates unresolved
- Public seating/inauguration versus Cup certified-winner rules, fusion affiliation, party changes, cancellation and sponsor discretion can break payoff equivalence
- NO asks are derived only from the same contract's YES bid where documented; distinct parties are never assumed complements
- Positive gaps, payout bounds and conditional pair diagnostics are not forecasts, expected returns or guaranteed arbitrage
