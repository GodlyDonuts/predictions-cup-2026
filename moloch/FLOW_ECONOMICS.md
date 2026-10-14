# Current flow economics — October 14

The immutable stream prefix for source 40 covers 19:51:04.837–19:53:06.053 UTC, 3,600 verified rows and 237 observed markets. `data/moloch-flow-economics-20261014-40.json` retains its source digest, final chain hash, market flows and separate YES/NO conditional thirty-second liquidation marks.

Market 342/exchange 1031 traded 416,206 shares. Its 55 prior-observable YES entry samples averaged −0.019273 per share; five NO samples averaged +0.007000. Each side occupies only two thirty-second execution buckets. Volume-weighted averages were −0.019802 and +0.005975 respectively. These are correlated conditional quotes, not observed fills or realized wallet returns. The favorable NO sample is too small to establish a strategy. Joining both sides indiscriminately would expose the wallet to the adverse YES flow.

The existing exchange 922 had no new trades in this prefix. Its active forward wallets remain unchanged. No market rotation, additional account collector, funding reset or contest order was introduced.

A separate prospective discovery hypothesis is frozen at registration: exchange 1031, NO join quotes only, using future executions strictly after the registration cutoff. Assess those with the existing three-second prior-observation requirement and later thirty-second executable one-share mark. Report all qualifying samples and censoring, including losses; do not reuse development samples as forward evidence. This is a signal diagnostic, not a funded paper wallet. A favorable diagnostic would still require fresh full REST reconciliation and a separately registered funded forward policy before execution claims.
