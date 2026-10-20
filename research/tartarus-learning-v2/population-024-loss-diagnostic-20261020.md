# GPU024 checkpoint: allocation loss diagnosis, October 20

The first completed fresh candidate remains negative. Its original CUDA evaluation returned worst-case marked ROI -10.86959%. A CPU-only diagnostic of the same immutable checkpoint returned -14.9729675% ordinary and -25.66936% stress. CPU sampling differs from CUDA: these are additional development trajectories, not reproductions or replacements of the original result.

The diagnostic reconciles terminal wealth exactly to cash, collateral liabilities and marked inventory. Both trajectories spent virtually all initial 100,000 cash on purchases. Ordinary recorded 75 fills, stress 63; every recorded fill was a purchase, with zero realized FIFO profit and no closing sales. Ordinary exchanges 952 and 971 account for 11,902.1525 of the 14,972.9675 marked loss (79.49%). Stress exchange 961 accounts for 20,137.2 of the 25,669.36 loss (78.45%). Those concentrated losses are on fresh terminal marks; stale holdings exist elsewhere and remain explicitly flagged.

This rules out unused capital or too few fills as the explanation for these diagnostic losses. It does not establish whether execution spread, subsequent price movement, inadequate observations or the action parameterization is the dominant cause. A separate CPU fill-time attribution run is in progress using the original frozen simulator, with instrumented/uninstrumented behavior comparison. It does not initialize CUDA, optimize weights, access independent testing, or contact an account API.

The original GPU learner continues. Its inherited candidate advances virtual time much more slowly than the completed fresh candidate; at 20:06 UTC the run had 71,670 decisions and 656 optimizer updates, but this is work performed, not profitability. No checkpoint is independently qualified for production. Historical acquisition and prospective capture handoff continue through the single original account queue with execution STOP intact.

Evidence: [exact diagnostic cases](population-024-loss-diagnostic-20261020.json), [original completed evaluation](population-024-first-selection-20261020.json). The reward has not been changed.

## Completed ordinary fill-impact control, 20:13 UTC

The original-source CPU run completed all 1,678 decisions in the ordinary case, with exact instrumented/uninstrumented parity for action trace, finance, knowledge, RNG, hidden state and budgets. Total marked loss 14,972.9675 consists of fill-time mark impact -15,835.83 plus non-fill wealth movement +862.8625. All 75 fills had absent delivered SIG books; 68 had no cached external reference and seven had a Kalshi reference. This describes cached state at fill time, not proof of what the network used at its earlier decision.

The result supports costly uninformed execution as the immediate failure mode in this trajectory. It is not evidence that a forced minimum trade rate, full-capital deployment or longer WAIT would improve returns. Stress attribution is still running on CPU. [Ordinary evidence](population-024-ordinary-fill-impact-20261020.json). [Explicit timing diversity implementation](inherited-clock-diversity-20261020.md).
