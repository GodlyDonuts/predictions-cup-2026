# R26 natural stop: untouched pre-intent book expiry

Fresh service inspection at13:07UTC found PID0 and the original natural stop at **12:53:40.377UTC**, FUNDAMENTAL_NOT_ELIGIBLE. Last trading heartbeat12:52:13.232 is historical. This was not an operator pause. Any later continuing-operation statement based only on that heartbeat is superseded.

[Root-authenticated original evidence](root-verification.json):16files,5,531 raw sealed native rows483808..489338,21,748 full original dispatch rows; all382 prior fill tokens and the original five-generation prefix remain exact. The entire stopped journal prefix is1,595,581,816bytes/SHA79330c4692a78b6020f8109dc6e9794ce55986287e97adebf874cf4c6111cf7e. No independent financial Session replay or account request was performed by this reader.

The reviewed851NO BUY680@.875 quote15469 has no original dispatch rows or actual native operation. SDK queue is positively quiet:0globalunknown,activeNone,manualReviewfalse. Final target book489338 was source12:53:28.3159968/request28.118/receipt28.410/processed28.548. At the stop it was12,061.0032ms old by source clock,12,259ms by request and11,967ms by receipt, exceeding the unchanged10s policy.

The pre-intent physical check first calls check_bound/assess, which may decline with NOT_ELIGIBLE before the explicit PHYSICAL_BOOK_STALE check. The correction classifies either exact code only in that original pre-intent catch, and still requires the original exact quote, positively quiet queue, no intent/attempt/ACK/native binding/fill and actual current native-book absence. A current book, unknown acceptance, prior operation, other failure or post-intent failure remains fatal. No TTL, review, price, cap, source clock or old financial row changes. The cohort binding/spec/qid are preserved; the next attempt requires its existing authoritative workflow.

This correction and the measured COW/index changes still require the final checked release and owner recovery. The declared13:09–13:14baseline is aborted because this worker was already stopped. No nonexistent sample or profit is claimed.
