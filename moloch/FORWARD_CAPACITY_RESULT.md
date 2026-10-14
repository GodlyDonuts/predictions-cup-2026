# Forward maker capacity diagnostic

The verified forward segment of strict wallet 8 has two modeled fills, zero realized profit and 4.760 owned cost. Its latest checked fresh liquidation mark is −0.040. This excludes the queue wallet's separately recovered historical +0.070 gain.

Four inherited-bank replays (recorded, retained opposite packet, execution-age clock, and both changes) reproduce the same two fills and result in this prefix. The original baseline matches its complete snapshot. Retention therefore has no measured improvement here.

A full observed trade-through capacity sensitivity produces three fills and 158.270 owned cost, with −1.330 fresh liquidation P&L. Retaining the opposite quote still does not improve that result. Higher assumed fills amplify inventory exposure instead of establishing profitable turnover. This scenario is not installed in the live wallets.

The comparison preserves inherited cash, all original quote liabilities, execution/processing clocks, source gaps, fees, captured depth and write pacing. No new capital or actual order is introduced. Results are retrospective diagnostics, not untouched holdout or verified exchange fills; missing terminal marks remain missing. See `data/moloch-forward-capacity-20261014.json` and reproduce with `node moloch/research/forward-maker-20261014.mjs TAIL_DIRECTORY NEW_OUTPUT_FILE`.
