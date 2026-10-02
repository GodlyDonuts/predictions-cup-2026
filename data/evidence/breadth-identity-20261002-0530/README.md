# Cached identity supplement

The older scanner cache contains all **237 exact market titles and exchange/option mappings**, plus **79 structured contract nodes** with individual request/receipt times. This supplement projects those public fields and joins them to the immutable mixed-age bulk lead table. It adds no network requests. The original null-metadata export remains unchanged.

Discovery cache snapshot time is **2026-10-01 19:14:57.984 UTC**. Exact discovery request/receipt times are unavailable and remain null; cache snapshot time is not substituted for them. Structured nodes retain their own historical clocks. These descriptions are old and require current rule/identity verification. Party comes only from an explicit `Party Winner` structured node; race/stage/state/office come only from captured fields. No opaque ID or title is parsed to invent metadata. Unavailable structured descriptions remain null.

`settlementPredicate` is a cached structured contract description, not an authenticated complete current resolution rule; `settlementPredicateVerified` remains false for every row. Research comparisons must still check exact party/race/stage and resolution conditions before any action. Existing price collection timestamps, unknown engine quote age, non-executable flags and no-depth status remain intact.

Reproduce from the repository root:

```sh
node live-paper/identity-join.mjs --leads data/evidence/breadth-20261002-0525/leads.json --identities data/evidence/breadth-identity-20261002-0530/identities.json > reproduced.json
```

Stdout including its trailing newline must equal `joined-leads.json`. The manifest binds the existing bulk input, projected identity input, joined output and join code (CRLF normalized to LF for code only). Reproducibility from derived inputs does not independently authenticate unshared original caches. Original raw discovery/node responses and account records are not included.
