# Full delivered depth and the model's own planning state

The optional full-depth profile addresses two observation limitations without restricting order choices. The old policy sees best bid/ask and aggregate displayed quantity, which cannot identify a large order's execution cost. It also sees draft-batch length without its contents, and its recurrent update does not consume the sampled action directly. Those omissions make sizing and coherent basket construction harder than normal local API access would require.

`full_depth_observations=true` selects `FullDepthTape`, `FullDepthEpisode` and the separately identified `FullDepthPolicy`. Every original delivered bid/ask level is encoded on the complete official0.005 lattice:199 prices per side. There is no top-20 cutoff. The original40 features remain, with398 log-depth inputs added, giving438 features. Original integer quantities still govern fills/accounting; floating-point neural depth inputs are not replacement liquidity. Unreceived hidden-world books add no depth values. Existing source-age/invalidity flags remain visible.

Own draft orders become observation-only entity rows with exchange identity, side, quantity, price, market/limit status, expiration and order index. The last locally sampled action is also visible. These rows cannot become API action targets and consume no API admission. The captured immediate result status is a string copy, so a server-side dispatch-record mutation cannot reveal an outcome before its acknowledgment. Stream/read receipts remain the source of market information.

## Information and source evidence

Two books can have identical best prices and total quantity but different depth. In a declared fixture, buying150 shares costs75 versus90 SUSQies despite identical old observations. This is an information-loss example, not a trading return.

The real-source audit starts with237 hidden books and zero exposed ladder values. Eight simulated charged reads plus one stream-token write result in72 delivered books, of which8 are reconciled. All6,980 delivered price levels are retained, including126 levels in one book. The old40 columns remain unchanged at this audited pre-planning state. Full observation arrays grow from178,560 to1,955,232bytes, an important capacity cost. Cached observations measured5.41ms median in this single CPU audit; the cache follows immutable delivered-book identity and never reads hidden-world books. [Audit](full-depth-observation-audit-001.json).

## Architecture and learning path

The new architecture is `tartarus-raw-api-full-depth-family-first-gru-v3`, with58,157,799 parameters at hidden4096. It changes the input projection, not action heads or quantity/price/expiry domains. `migrate_full_depth_encoder=true` explicitly permits a development parent migration: all previous parameters are retained exactly in the original columns, new columns start at zero, and optimizer/old trajectories are reset. This preserves initial baseline behavior on an unchanged entity view within numerical tolerance; added planning observations intentionally change what later decisions can see. Checkpoint loading, frozen source identity, fused PPO, fast on-policy guards and population fresh mutations support the distinct architecture. [Full-size migration proof](full-depth-migration-001.json).

159 focused checks pass, including causal delivery, all199 ticks, large aggregate quantities, no changed order masks, local-plan visibility, unreceived-result privacy, checkpoint round trips, new-column mutation detection, real economic labels, fused updates and two software-fixture population cycles.

Two real-data GPU probes ran beside the existing trainer. One world/16decisions used1.50GB peak; two worlds/64decisions used2.10GB. Both accepted PPO updates and matched serial/batched financial states and clocks. The two-world probe collected in1.635seconds versus1.991serial, and updated in0.658seconds. Priming reads are explicit and charged within each independent simulated world. These are bounded prototype results, not proof of eight-world/128-step capacity or alpha. [First probe](full-depth-gpu-benchmark-001.json) / [second probe](full-depth-gpu-benchmark-002.json).

Phase014 continues with its original40-feature policy and copied frozen source. Its current worktree has future opt-in changes; that is recorded separately from the frozen source snapshot. No hot model replacement or production change occurred. A larger memory check and new pinned training profile are still needed to adopt the richer inputs. [Active run](active-learning-20261019.json).

## Relationship and remaining economic limits

The retained relationship census was completed October18 at23:34:50UTC, with zero active relationships or known collateral components. This dated observation does not establish current availability or eliminate the need for complete relationship/collateral simulation. [Dated evidence](retained-relationship-census-audit-001.json). Full collateral/component debt, contract metadata/lifecycle, official valuation coverage, full multi-day history, queue calibration and independent positive ROI remain outstanding. The active population's completed development results remain negative; broader inputs do not establish a winning model.
