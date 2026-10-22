# R97: serve current monetary decisions between ordinary packets

The one-filled-order-per-minute objective remains unmet. The original-receipt audit at 16:02 UTC found two distinct filled orders since R94 installation and 18.73 SUSQies incremental realized profit. This is not whole-account ROI. Partial receipts count as one filled order. See `r96-live-turnover-20261022.json`.

R96's file-only controller now selects the three current-price models while retaining both historical-envelope stresses and their original source provenance. A fresh three-model review was published at 16:00:57 UTC; the production worker deferred it at 16:02:22 because existing physical orders required reconciliation. Its conditional expected value is not realized profit. The previous all-five controller was stopped without restarting the account worker. See `r96-controller-started-20261022.json` and `current-price-policy-r96-20261022.md`.

R97 addresses that execution delay:

- A fresh positive monetary review can yield an exactly owned ordinary BUY or SELL through the existing cancellation packet. Package, unbound, unknown, already-canceling and reviewed-rank orders are excluded. Cancellation acknowledgment does not release funds; terminal reconciliation is still required.
- The same reviewed-order service runs between ordinary sweep passes after current reconciliation. A dispatch mutation ends the sweep so an earlier permit cannot survive it.
- The native research view is exported after each completed ordinary packet rather than only after the whole sweep. Normal report flushing remains authoritative.
- The already-qualified R95 compact pre-network proof runtime is included; the native engine and original financial history remain unchanged.

Qualification: 270 worker tests pass, with 149 inherited unchanged runtime tests. The exact release SHA and source hashes are in `r97-rank-service-qualified-20261022.json`. The release is qualified offline at publication time; installation and fresh execution need separate evidence. No limiter, activation window, monetary cap or basis reset is part of this change. An exchange fill cannot be guaranteed by scheduling changes.
