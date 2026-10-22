# R80: route managed inventory through its current exit policy

R80 is qualified, not deployed. R79 remains the sole production worker. The one-distinct-first-filled-order-per-minute target remains unmet.

Fresh R79 diagnostics show retired October 17 native maximum-age quotes 14338 (865 YES) and 14340 (1051 NO) repeatedly receiving priority read packets on October 22. They end with `EXIT_REFRESH_NO_FRESH_CANDIDATE`. Meanwhile the worker reports ten current reviewed exit hints. [Original observations](r79-managed-age-blocker-20261022.json).

The dispatcher previously admitted every historical `maximumAge` quote before checking whether its inventory belonged to a managed cohort. A passive reviewed exit made the separate refresh eligibility predicate true, so this legacy nomination could preempt the current reviewed queue despite being unable to create the required reviewed reservation. R79's sweep then terminated when the same priority cursor repeated. It had correctly examined five candidates in one observed cycle, but the obsolete queue remained a blocker.

R80 excludes managed inventory from that historical nomination path. Current reviewed deadlines, source revocations, profitable exits and book-refresh nominations continue through their existing dedicated path. Unmanaged age exits retain their prior checks. The change does not alter a sale price, quantity, original FIFO basis, unknown-acceptance resolution, account limiter or monetary control.

All 239 worker checks pass, including a regression that prevents managed historical-age nominations from consuming queue work while retaining current reviewed exit nominations. The optimized executable loads. Runtime and native libraries are byte-identical to R79; only worker source changed. Binary SHA256 `94fc09256728e09aed80f78775220446222641fdcdd820f55858fbe3f0572721`, worker SHA256 `da5bcd0cb7e600e1bb33adf381f1cdcd4d1f65f74de5fcfd93c868c989559714`. [Qualification](r80-managed-age-qualified-20261022.json), [reproducible build](tools/build-managed-age-r80-20261022.py).

R79's original-receipt audit at 01:26:22 UTC confirms one new filled order since startup: 558 NO1077 shares bought at 0.855, first filled 01:19:59.288. Its two partial receipts count as one order. The purchase used 477.09 SUSQies and produced no realized profit. Total receipts rose from 493 to 495, with original FIFO and finances reconciled. [Audit](r79-turnover-20261022.json). Separate official receipts at 01:25:28–30 mark the account at 94,232.46 SUSQies, down 5.76754% from 100,000; these are not executable liquidation or an atomic snapshot. [Marks](r79-latest-account-20261022.json).

The qualified correction does not yet establish a fill-rate or ROI improvement. Deployment must preserve the running worker's original history and reconcile outstanding orders; subsequent live evidence must show actual exits and new fills.
