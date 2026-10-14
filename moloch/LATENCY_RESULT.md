# Activation latency and complete trade-cycle economics

Nine funded comparisons replay original episodes 21–27 on exchange 1051, which remains a financial read scope throughout those episodes. They compare 3,000/1,500/500 ms activation delays at 1%/10%/100% participation with 500-share packets, retained hedges without overshoot and execution-time inventory aging. All original baseline snapshot finances are reproduced exactly before evaluating alternatives. Read/write pacing, depth, fees, minimum quote age and unresolved acceptance rules remain fixed.

Each delay produces exactly one purchase and zero realized profit at each participation level. Remaining costs are 4.320/44.640/80.000; last fresh liquidation P&L is −0.270/−2.790/−5.000 at 08:22:18.686 UTC. The three-second variants end with two unresolved quotes; faster alternatives retain three or four. No activation-delay improvement establishes profitable turnover.

Keeping the scope in every manifest fixes retirement-related absence, but does not fill the temporal outages or authenticate missing alternative quote windows. Complete tape coverage and later executable depth remain necessary to release liabilities and close inventory. The final liquidation mark is missing and no variant qualifies for ranking. Hypothetical faster activation is not a measured production latency. Live policies are unchanged.

Reproduce with `node moloch/research/latency-study-20261014.mjs NEW_OUTPUT`. Evidence: `data/moloch-latency-study-20261014.json`. The current fixed-scope forward experiment on exchange 1027 remains active and covers both actual alternative-wallet liability sets through its recovery watcher.
