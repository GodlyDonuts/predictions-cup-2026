# R53: restore caching for large Tartarus evaluations

The second fresh reviewed rank sale, q19565, reached native reservation but was deferred before any network attempt at01:09:05UTC October19. Its original cohort binding began01:05:57; expiring physical facts did not survive the subsequent checks. Original q19556 likewise has only a not_sent dispatch record, no official order or fill. Both remain irreversibly retired.

The actual19,565-row cache measured75,414,376 bytes under the old conservative bound versus16,977,737 bytes of actual encoded order rows. Multiplying every ordinary string/key byte by six disabled caching above64MiB. Ownership checks then repeatedly rendered the entire order history. This is a demonstrated disabled fast path; the full measured production improvement still requires deployment evidence.

R53 counts actual serde_json string escaping without allocating a second encoded copy. ASCII/control/quote/backslash/Unicode handling is checked in both keys and values.100,000-record and64MiB retention bounds are unchanged; oversize inputs still work uncached. Session identity, manifest and durable-head invalidation remain unchanged. No financial checks, ledger, native engine, SDK state machine, risk limits, source ages, STOPs or daily windows change.

All271 ARM64 library checks pass (two existing ignored), including five order-cache checks and the real Session cache-invalidation fixture. The optimized ARM64 release built successfully; its compiled Rust inputs are checked against main. [Qualification](checked-production-lane/r53-qualified-release.json). R52 stays running until a positively quiet same-owner deployment boundary. This document does not claim R53 deployment, a Tartarus fill, profitability or a production speedup.


At01:29:39UTC the qualified release was installed as sole worker871596, actual executable/arguments/startTicks34303358 verified. Only the binary binding changed in the original control. The stopped boundary had395fills, all actual acknowledgments terminal, SDKactiveNone/unknown0,20 unchanged engine bridges and14 original G12 reviews. No STOP was cleared, financial history edited, key reset or daily/risk/source clock renewed. [Actual installation](checked-production-lane/r53-installed-restoring.json). The new worker is restoring; fresh trading and a rank fill remain unverified.
