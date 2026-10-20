# Historical observations now reach trainable neural inputs

The new `tartarus-historical-information-gru-v10` architecture consumes delivered SIG candle, Kalshi trade and Polymarket sample records under a distinct observation protocol. Each record becomes an information token with source/receipt ages, venue and target identity, price-presence flags, prices, volumes and trade metadata. Missing prices remain explicitly missing. Actual present-day retrieval dates are provenance only, never historical actor features.

Every record in a delivered page is represented. Tokens remain stable across repeated observations until the actor uses that observation, then recurrent state carries the information forward. A paid re-query exposes the page again. Receipts arriving during inference remain pending for the next decision. This avoids copying all old pages on every decision; it does not cap legal decisions or reset accounts, inventory or time. Historical records have no order-action masks and never become executable depth.

`Experiment` now supports an explicit `historical_information` binding with separate train/evolution/test source specifications, pinned manifests, declared delivery schedules, information lower bounds and reference mappings. The three venue readers share resources within each role and close with the experiment. No test source is opened when running training or development selection. New history actors require fresh founders; old checkpoints are not silently relabeled.

## Actual validation

The real-source CPU integration registered 787 historical targets: 237 SIG, 234 Kalshi and 316 Polymarket assets with conditional same-party mappings. The remaining 62 Polymarket context assets are not yet mapped into this binding. Mapping historical availability remains unverified; neither pricing authority nor settlement equivalence is claimed.

Three diagnostic paid reads delivered 1,501 records into the 517-column actor observation. Four subsequent policy decisions produced one accepted PPO update, changed the history encoder columns, and saved a checkpoint that loaded with identical weights and the new architecture. Reads were explicitly driven for this integration check; it does not prove that a trained policy chooses useful reads. [Actual checkpoint and experiment hashes](historical-neural-20261020.json).

Six neural tests cover all 1,000 rows of a page, missingness, candle-close causality, future-row rejection, retrieval-date exclusion, receipt consumption races, changed input weights, checkpoint identity and exact closed-episode replay. Three binding tests cover role restrictions, required venues/source hashes and resource cleanup. Together with population/protocol regressions, fifteen cases pass. The earlier scoped-execution and named-target regression checks also passed; an unrelated nonexistent test-module name produced a loader error and was corrected to the actual protocol suite.

## Training state and remaining work

The original GPU022 stopped after 49,152 decisions with zero committed updates, empty stderr and no captured exit cause. Its artifacts remain intact. GPU023 is being prepared with fresh founders and streaming PPO updates within the same continuous ten-minute development paths; full fifty-minute development selection remains unchanged. Sixteen decisions organize a memory chunk, not a decision budget. Bootstrapped critic targets can be inaccurate and require evaluation; changing optimizer scheduling is not evidence of better ROI.

This remains development on the October 19 executable-book dataset with older public archives available as historical context. Full chronological historical execution replay, the complete multi-day reward run and independent profitable ROI are still unestablished. The reserved October 21–25 prospective test remains untouched. Production financial controls are unchanged.
