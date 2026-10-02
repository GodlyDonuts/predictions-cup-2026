# Existing bulk observations for lead generation

The export covers all 237 catalogued exchanges using the latest successful existing capture for each of three API batches. As of the captured journal clock **2026-10-02 05:25:17.604 UTC**, the batches were received at **04:37:37.522**, **04:22:17.239** and **04:29:24.107 UTC**. These observations are already roughly 48–63 minutes old by collection time. Engine quote time is absent from the official bulk schema, so actual quote age remains unknown. This is a mixed-age cache, not a simultaneous current quote table.

`leads.json` includes market/exchange IDs, option, observed price/bid/ask/spread where present, exact request/receipt times, collection age, null source quote time/age and explicit non-executable/no-depth flags. Party, race, stage, title and settlement predicate are unavailable in the local ID catalogue and remain null. All 237 rows were present in these batches; missing/not-observed states remain supported by the exporter.

Reproduce from the repository root:

```sh
node live-paper/breadth-export.mjs --snapshots data/evidence/breadth-20261002-0525/snapshots.json --universe data/evidence/breadth-20261002-0525/universe.json > reproduced.json
```

The stdout bytes, including the trailing newline, must match `leads.json`. `manifest.json` binds input/output byte counts and hashes and all transitive code hashes, with CRLF normalized to LF for code only. Use the commit containing this bundle; the new exporter is bound by its hash and the base commit identifies preceding dependencies.

The inputs are derived normalized market-only snapshots, not the original journal. Their reproducibility does not independently authenticate the unshared original capture or account context. No raw account records, auth headers or credentials are included. Exporting and reproducing require no API calls; the existing serialized collector and pacing are unchanged.

Join to independently sourced exact market identities and settlement predicates before comparing external books. Large discrepancies are leads requiring fresh rules, timestamp/depth, contract equivalence and cost/risk verification. Neither bulk nor external prices alone are fair probabilities or executable opportunities.
