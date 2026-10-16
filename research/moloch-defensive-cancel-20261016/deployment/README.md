# Single-owner deployment and actual fresh trading

The checked b112c6c1 release, binary3ab6ef7c, started sole SIG PID525450 at
17:07:56UTC after518852 stopped cleanly. The original startup verification is
retained in live-verification.json and explicitly reports fresh trading false.
The later [17:26 proof](fresh-live-verification.json) and
[17:30 proof](fresh-live-verification-1730.json) establish fresh PID-bound trading
progress, healthy realtime, matching executable and no STOP. At17:30 there are
five new official order acknowledgments and two reconciled fill rows.
The full-cycle status still identifies the predecessor; it is not used as proof
of successor freshness. The report seal is independently checked.

The two rows sell364+824 YES shares at0.340 against0.335 original cost, closing
1188 residual shares and realizing5.940000 at zero fees. The inventory was
acquired before the restart. These receipts prove an actual profitable exit,
not new-entry alpha, general profitability or economic benefit from defensive
cancellation. The finite source-bound lifecycle audit is separate.

The preserved bank reports200 cumulative fill rows, cash72,716.935000,
owned cost22,731.515000, ordinary realized−176.595000 and inherited gross
+45.130000, totaling−131.465000 SUSQies. Full capital95,579.915000 includes
55,579.915000 attributed contributions, not trading profit. Native8acc, the bank,
twelve bridge bytes and daily windows were preserved. `defensiveCancel:true`
activates the checked optional lane; an order acknowledgment alone never proves
terminal release or a fill.

Read-only V4 PID525623 started17:25:52 with original reviewed200ab producer and
map3f8d against525450's new genuine breadth journal, finite3600s. The
[start receipt](trace-v4-start.json) verifies unchanged SIG PID/start identity.
Old V3 remains normally sealed. Observer cohort changes do not restart SIG.

The [raw artifact pins](fresh-artifact-pins.json) bind these original checks and
the [same-protocol delay probe](../measurement/README.md). Earlier startup/stop
approval, build and rollout evidence remain separate and unchanged.
