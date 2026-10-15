# Preserved cohort drain

`run.mjs --resume PARENT --drain` changes only `entriesEnabled` to false before consuming new receipts. It records prior/next policy hashes and retains exact cash, fills, FIFO inventory, order IDs, funding, fees and limits. Existing resting buys can still receive delayed fills within their original windows; they are not deleted or assumed cancelled. Covered exits remain available after pledge reconciliation. Both the completed-tail verifier and fill auditor replay the same transition.

The current cohorts need full economic evaluation: the queue wallet's historical +0.070 realized gain still has inventory, and the newer strict wallet's two fills have negative fresh liquidation marks. New entries would confound that check. The next source therefore preserves the banks and disables fresh acquisition while continuing full tape/book recovery and covered inventory management. This is an operational paper drain, not a pause or completion of the competition goal.

The actual serialized collector can retire empty former scopes only after all three wallet views prove they have no liabilities there. One remaining scope reduces its normal book/tape cycle to two paced reads. The global websocket still supplies discovery observations. Budget, DNS recovery, immutable history and zero actual orders remain unchanged.

117 Node tests pass, including preserved financial fields and completed-chain reconstruction with both entry-scope and drain transitions. The full-participation/retention forward comparison remains rejected; draining is not a profitability certification.
