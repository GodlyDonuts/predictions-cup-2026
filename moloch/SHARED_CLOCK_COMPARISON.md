# Forward exit comparison

Phase55 completed24 official REST receipts and zero new fills in either funded100,000-point paper bank. Separate consumer startup nevertheless changed the first entry order, timestamp and size. This invalidates interpreting that phase as an isolated exit-policy comparison. The original evidence is preserved in `data/moloch-fixed-offer-timing-confound-20261014-55.json`; no cash gain was backfilled.

`run-paired-exits.mjs` now runs both existing journal consumers in one process. Each reaches a barrier for the exact same event/index before either advances its financial state. The barrier assigns one actual wall-clock timestamp after both are ready. It rejects conflicting events, duplicate participants, and clocks earlier than the receipt; failure releases the waiting consumer. This preserves source receipt and trade execution clocks, rather than backdating decisions to make quotes appear earlier.

The wrapper verifies the original terminal parent, registered policy hashes and equal policy fields other than inventory exit mode. Both histories retain the normal snapshot, intent, journal, source-prefix and terminal verification format. Tests exercise the actual journal consumers: they persist identical event clocks and initial entry intents. Neither the clock barrier nor the wrapper adds an account client or submits an actual order.

Usage after registering a new source and policies:

```
node moloch/run-paired-exits.mjs REGISTRATION CONTROL_OUTPUT CANDIDATE_OUTPUT FIXED_EXIT_POLICY
```

Stop both consumers only after they have consumed the same complete source. Reconcile every funded quote and position through later observations before treating a phase as closed. The comparison checker excludes inherited gains, verifies the same complete source/events, and reports maximum clock skew and unresolved inventory. A successful timing test is not evidence of profitable market making. Fixed offers still require new filled and closed forward cycles, realistic fees, adverse selection and capital scaling evidence.
