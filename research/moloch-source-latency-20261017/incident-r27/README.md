# Original R27 pre-intent incident

The finite local verifier passed against the original 6,054,486-byte incident ZIP SHA256 `7ee0a95b75500d732e2568a1962eb1491cd525125284a495f39bc14d38666af4`. All eighteen original artifact pins were checked. The initial handoff's duplicated `a` in this hash was a transcription error; root acknowledged this actual 64-hex raw hash. No account requests, Session/financial replay, runtime writes, or model changes were performed.

[aggregate.json](aggregate.json) records the authenticated suffix from R26 anchor489338 through R27 head492481, all raw sealed rows, exact final head/report/manifest identity, complete dispatch chain, and exact preservation of the preceding 382 original fill tokens. The comparator is the original node-continuity live proof (`19e964fb2168bee447600a23e14cdb83a1dd5ead24be04a0c43cd0f824ba0774`); its complete fundamental prefix is retained and the incident has exactly six review generations. Anchor bytes are checked against the stopped R26 incident (`8c09fd1aaa589dd0e4e55da09188b5b5aa7f19c9ea3378c779155da534dc02d3`).

Quote15612 is a native, unretired BUY680 NO/ex851 at `.875000`, filled0. It did not exist in the earlier comparator, has no original dispatch row and no actual-operation event in the authenticated intervening native suffix. PID654706 stopped with `FUNDAMENTAL_NOT_ELIGIBLE` at14:21:28.344 UTC. The captured SDK queue reports zero unknowns, no active operation and manual review false. The final admitted ex851 book's original source, request and receipt clocks are all more than the actual ten-second bound before STOP; exact clocks/ages appear in the aggregate.

This establishes the recorded pre-intent state and book-age condition; it does not independently identify which internal eligibility subpredicate returned false. A captured whole-prefix fingerprint is labeled reported evidence: the large whole prefix was not independently rehashed here. Raw source-body membership is not established merely by a native source reference. There is no claim of an exchange submission, fill, new loss, model calibration, profitable counterfactual or execution permission.

Reproduce from the repository root, supplying private immutable originals and a **new** output path:

```text
python research/moloch-source-latency-20261017/incident-r27/verify.py --incident ORIGINAL_INCIDENT_ZIP --node-proof ORIGINAL_NODE_CONTINUITY_ZIP --anchor-incident ORIGINAL_R26_INCIDENT_ZIP --output NEW_AGGREGATE_JSON
```

The verifier pins and reuses the existing raw-seal/Packet checker (LF SHA256 `1fbc599e16417778d4aa48545eb2c7af9c3073875fd4ffb26f72c0333443a34b`). Run normally, without Python `-O`: its explicit assertions are part of validation. Original private packet bodies are not committed; only native quote identity, public book clocks, hashes and derived counts are published.
