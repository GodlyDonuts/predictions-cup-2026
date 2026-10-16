# Original sealed input replay: first actual cohort

V2 captured 16:24–16:31 UTC, then stopped normally with its original terminal seal. The raw producer was `200abfd0`; the next finite, read-only V3 cohort started PID519257 without changing SIG PID518852 or its process start identity. Both producer code and trace protocol were already on main32c4704f before activation.

Root pinned the exact input manifest at 16:36:12 UTC before opening replay outcomes. Original compressed trace117,051,357 bytes, sealed samples2,378 bytes, producer, mapping and separately retained original SIG segment prefix are individually pinned in input-manifest.json. These large detached originals remain on the VPS; this public directory publishes their actual hashes, lengths and results, not a substitute dataset.

Nineteen synthetic checks pass locally and on Linux. Two initial Linux packaging runs lacked existing test fixtures; their original logs are retained. An initial-bookmark append-race correction preserves the original cursor when the source grows between open and bookmark; economic sampling, clocks and limits were unchanged.

The first actual replay ran 16:41:26–16:43:30 UTC (124.208 seconds) and passed: 3,213 trace records, 488 cache polls/applies, 386 SIG callbacks and both original emitted rows reproduce exactly. There is one horizon label. No failed cache poll or segment rotation occurred in this cohort. Exact original cache-body and consumed SIG-byte membership and the sampler’s first consumed horizon-book choice are verified.

This verifies the known sampler and its captured suffix. Full history, provider trade completeness, collector processing/epoch, tournament-topic binding and resolution equivalence remain unverified; wholeWitnessVerified, eligibleForModelValidation and executionAuthority remain false. One label cannot qualify a profitable predictor. No fitted model or trading activation follows from this result.
