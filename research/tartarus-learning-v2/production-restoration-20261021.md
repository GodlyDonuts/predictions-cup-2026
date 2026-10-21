# Production restoration and account bridge — October 21

The human explicitly instructed this thread to restore production immediately. This supersedes the earlier learning-only restriction on changing production state. It does not make the experimental ML population profitable or authorize fabricating account history.

## Restoration

The sole production R57 service was inactive. Its original October 19 STOP recorded `STREAM_CONNECT_ATTEMPTS_EXHAUSTED`; the retained recovery record identified `STREAM_DNS`. The data-only capture controller was stopped through its own STOP protocol, releasing its SDK lock and restoring the original empty network marker. No concurrent SIG client was started.

The original checked R57 binary (`120bdb492c5f87651a3333dbcb28c73fceb025c4ac034db44bb481e476333658`) and control (`bed178634e699f969a344bb3c1aa3a595c37ef40db1198ee9935a87cce00467a`) were verified. The same binary completed read-only account preflight with the financial STOP still present. The existing SDK reader verified its original checkpoint and journal; after preflight its active request was null, unknown acceptance empty and manual review false. Official open orders were empty.

The exact resolved DNS STOP and network marker were archived under `/home/archon/moloch-production-restore-20261021-001`, with the original control, bank manifest and dispatch length bound in the restart record. The financial ledger, holdings, risk caps, recovery budget and intent identities were not reset. No experimental ML checkpoint was deployed.

The existing systemd service started successfully as **968525**. At 00:38:46 UTC it was still restoring the original 2.4 GB financial ledger. **Fresh current-process trading, realtime health, orders and fills are not yet verified.** The old progress file belongs to 886899 and must not be used as new-worker evidence.

## What the account decline actually means

Fresh official preflight at **00:37:57 UTC** and the retained native report at October 19 14:15:46 reconcile all **24 held sides** exactly by exchange, side and quantity. No intervening order or fill is inferred.

| Item | SUSQies |
|---|---:|
| Current cash | 403.36 |
| Official marked holdings | 94,296.23 |
| Cash plus official marks | 94,699.59 |
| Difference versus original 100,000 | −5,300.41 |
| Native remaining FIFO principal | 95,339.055 |
| Native attributed realized gross | +162.500 |
| Of which new trading / inherited management | +86.225 / +76.275 |
| Native fees | 0 |
| Opening cash plus verified inherited lot principal | 95,579.915 |
| Opening principal difference versus 100,000 | −4,420.085 |
| Current marks minus native remaining FIFO | −1,042.825 |

Exact accounting bridge: `403.36 + 95339.055 = 95579.915 + 162.5`. Current marks are another 1,042.825 below remaining native principal. Thus the total −5,300.41 difference bridges as `−4420.085 + 162.5 − 1042.825`.

This is a principal/mark reconciliation, **not a measured return since takeover**: opening lot principal is not opening market valuation. The earlier principal deficit is not newly learned ML loss. Native inherited basis and official reported cost basis differ by 322.255; the API's rounded unrealized loss of 720.55 therefore must not be substituted for native FIFO mark loss. Neither number establishes executable liquidation value or final settlement P&L.

Most capital is invested, rather than idle. A demand for more turnover does not create available cash or an economic edge. Production management must consider these existing positions, exact executable depth and the inherited basis.

## Research separation

Population030 continues on its frozen pre-independent dataset/source. Live account oversight and this restoration are production operations, not new training or selection inputs. October21–25 remains unconsumed by learning. Stopping the standalone collector introduces a prospective source interruption; continuous held-out coverage is not claimed. Full ML daily-reward training and independent positive ROI remain incomplete.
