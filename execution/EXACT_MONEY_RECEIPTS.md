# Exact official money receipts

The existing serialized collector can preserve selected JSON numeric lexemes before conversion to IEEE doubles. `exact-money-receipts.mjs` wraps its existing fetch decoder; it creates no network client, admission loop or extra request. Only the matching official v1 tournament response and scoped single-order acknowledgement are eligible. Other routes and errors pass through.

The native numeric response still goes to the existing YES core. Selected `myBalance` and `totalCost` tokens produce exact micro-decimal strings only when the source representation supports them without rounding. Nonzero sub-micro precision, scientific or negative money remains explicitly unresolved. Pending enrolment is recorded so a seed balance cannot silently become live cash evidence. A failed acknowledgement binding does not discard the response or its returned ID: the execution core must reconcile it.

The restricted runtime journal contains only selected money, scoped order identifiers, canonical side/action, request/body hashes and receipt time. It contains no body, key, header or unrelated account fields. Duplicate JSON keys, including escaped equivalents, reject. The immutable receipt chain is fsynced and checks sequence, prior hash, file identity and STOP before every append. Persistence failure blocks delivery through the collector and requires reconciliation; it never authorizes a retry.

These receipts do not establish account-activity completeness, fill-price convention, gross/available collateral equivalence or authoritative reconciliation. `no-order-economics.mjs` still needs a full isolated current-order lifecycle, signed fill and position evidence. Do not manufacture that evidence from dashboard projections.

Run `node --test execution/exact-money-receipts.test.mjs execution/no-order-economics.test.mjs live-paper/runner-lifecycle.test.mjs`.
