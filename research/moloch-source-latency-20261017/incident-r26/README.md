# R26 natural stop: untouched pre-intent book expiry

Fresh service inspection at13:07UTC found PID0 and the original natural stop at **12:53:40.377UTC**, FUNDAMENTAL_NOT_ELIGIBLE. Last trading heartbeat12:52:13.232 is historical. This was not an operator pause. Any later continuing-operation statement based only on that heartbeat is superseded.

[Root-authenticated original evidence](root-verification.json):16files,5,531 raw sealed native rows483808..489338,21,748 full original dispatch rows; all382 prior fill tokens and the original five-generation prefix remain exact. The entire stopped journal prefix is1,595,581,816bytes/SHA79330c4692a78b6020f8109dc6e9794ce55986287e97adebf874cf4c6111cf7e. No independent financial Session replay or account request was performed by this reader.

The reviewed851NO BUY680@.875 quote15469 has no original dispatch rows or actual native operation. SDK queue is positively quiet:0globalunknown,activeNone,manualReviewfalse. Final target book489338 was source12:53:28.3159968/request28.118/receipt28.410/processed28.548. At the stop it was12,061.0032ms old by source clock,12,259ms by request and11,967ms by receipt, exceeding the unchanged10s policy.

**Callgraph correction at14:04UTC:** the exact NOT_ELIGIBLE arises in bind_quantity after assess_quantity declines. Worker832 calls bind_quantity before its physical_decision handler; check_bound does not generate this same entry code. R27 therefore did not fully catch the original incident path, despite passing helper tests. Its source/index improvements are operating; a minimal combined original-bind/physical-decision handler and actual-bind regression are being prepared. Original exact quote, positive quiet queue, no intent/attempt/ACK/native binding/fill and actual current native-book absence remain mandatory. No TTL, review, price, cap, source clock or old financial row is changed; original errors after intent remain fatal.

This correction and the measured COW/index changes still require the final checked release and owner recovery. The declared13:09–13:14baseline is aborted because this worker was already stopped. No nonexistent sample or profit is claimed.
