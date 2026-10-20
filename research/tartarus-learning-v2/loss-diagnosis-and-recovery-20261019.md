# Original-source loss attribution and GPU018 recovery

**Current:** GPU018 is training from checked source `4ae154396a3649c4cea25b2cd150ed894806c328`, actual worker23000/launcher43880. Fresh accepted learning, original source pins and every transferred founder's parameter fingerprint have been checked. GPU017 stopped on a CUDA backward out-of-memory error after977 accepted updates. Its last durable active-member snapshot was960:17 accepted updates after that snapshot were lost. This is a weights-only recovery, not a completed-generation handoff or financial-world resume.

All eight members remain: six completed/evaluated checkpoints, member6's latest durable accepted snapshot and member7's existing founder. Their tensors match the new founders exactly. Optimizers and simulated episodes start fresh; prior results remain in the original run and are not relabeled as new profits. GPU018 uses eight worlds with32-decision compute chunks, an80% CUDA allocator fraction and rotating snapshots every16 accepted updates. Compute chunk size changes memory/optimization organization, not the allowed decisions or API/portfolio domain. A real eight-world32-step capacity trial completed accepted PPO updates before recovery launch. [Exact recovery, founder fingerprints and capacity evidence](population-018-oom-recovery-001.json).

The failed controller6928 had correctly refused an unexpected stopped trainer. No completed generation or offspring was fabricated. The recovery instead used the existing verified unfinished-population path after confirming the terminal OOM. Worker23000 is bound to its exact command/config/run and Windows creation identity. The initial new-run calibration is24ms; the diagnostics below preserved the **original27ms** calibration.

## What the completed candidate actually lost

Diagnostics ran against the original frozen GPU017 policy/simulator source `d8fbf96a8db3e91c2e938cbf36ff6df18f9c40a7`, original checkpoints, source calendars, case seeds and calibration. Every instrumented prefix was compared against an uninstrumented control: full action trace, finance/fill history, final knowledge, hidden state, RNG and budgets matched. No optimizer or account client ran in those diagnostics. Source-run and checkpoint hashes are in the artifacts.

The decomposition uses exact twice-micro-unit arithmetic. Fill-time wealth change at that instant's SIG midpoint convention includes spread/depth price impact, modeled adverse execution and fees. The residual is all non-fill wealth change, not proven forecasting alpha. Borrowed principal is subtracted as a liability; complementary redemption and failed atomic baskets conserve the original ledger. Unavailable marks remain unattributed rather than becoming zero cost. These midpoints do not establish executable liquidation or settlement profits.

Member1 was the least-losing of six evaluated members, with worst full-case marked development ROI-10.84499%. Its inspected replay showed:

| Case | Simulated seconds | Fills | Marked P&L, SUSQies | Fill-time mark impact | Non-fill residual | Whole original case? |
|---|---:|---:|---:|---:|---:|---|
| First calendar, nominal |22.053|40|-6681.56|-6681.56|0|No|
| First calendar, stress |27.407|18|-6926.9975|-6926.9975|0|No|
| Second calendar, nominal |1499|27|-8596.725|-8347.4475|-249.2775|Yes|
| Second calendar, stress |48.901|42|-10914.4225|-10914.4225|0|No|

All127 fills were crossing-book fills. At fill time the exchange's delivered SIG book was absent for every one.126 had no cached mapped Kalshi/Polymarket reference; one had a mapped Polymarket record. This is fill-time cache evidence, not proof of exactly which information was present or used at the earlier decision. The actor may also have had trade prints, its own ACKs and learned historical information. The observed problem is expensive aggressive execution with little delivered quote context, not a demonstrated lack of all information. [Exact member1 controls and attribution](original-loss-attribution-002.json).

The first nominal prefix turned over96692.98 SUSQies from a fresh100000-SUSQie paper account in22.053 simulated seconds, losing6681.56 against contemporaneous midpoint marks. Modeled fees were0; there was no net non-fill marking change in that prefix. Stress added960.475 and1633.275 SUSQies of modeled adverse cost in its two prefixes, within the reported total fill impact. These are diagnostic paper outcomes, not actual account transactions.

Member4's first256 decisions produced no fills, so that short prefix could not explain its full-case loss. A longer4096-decision control of its worst stressed case reached117.298 simulated seconds and11 crossing fills: marked loss4805.6675, all fill-time impact, including692.31 modeled adverse cost and0 fees. Eight fills had a valid delivered book and three an invalid one; none had a cached mapped external reference. Thus merely receiving a book will not by itself fix the pricing policy. This prefix is not the full case, whose prior marked loss was46.7737325%. [Longer member4 control](original-loss-attribution-003.json).

The code audit also identifies an API representation weakness worth a controlled experiment. Read tokens currently encode their full endpoint key through an unrelated hash, while contract tokens encode a different contract hash. Endpoint type and the shared exchange/market link are not consistently explicit model features. A normal API user has those identifiers and operation names. Exposing that already-public metadata could make information acquisition easier to learn, without supplying hidden prices, forcing reads, forbidding aggressive execution or changing any legal action. Pricing behavior still needs a separate learned improvement; this hypothesis is not yet a profitability result or an activated strategy change.

## Failure, safeguards and verification limits

The CUDA OOM occurred during a period with concurrent GPU diagnostics. Those diagnostics may have contributed; they should not have shared the training device. The error also reported substantial reserved/unallocated allocator memory, so sole causality is not established. All GPU diagnostics were stopped before capacity verification and recovery. Current diagnostics default to CPU and check for active ML/compute processes before importing Torch when CUDA is requested. A live CLI check refused CUDA while worker23000 was running and created no diagnostic output. This preflight is not a distributed device lease; do not schedule GPU diagnostics during training or handoffs. Existing older frozen tools are not retroactively guarded.

Ten attribution tests check conservation, fees/stress, drift, borrowing, redemption, rollback, missing marks, observational equivalence and information-timing labels; three guard tests check resource refusal. A CPU-only full suite passed343 tests in68.364seconds with four CUDA cases intentionally skipped. An earlier attempt to hide CUDA through visibility masking alone gave inconsistent Windows availability/device-count results and failed three CUDA-specific cases; it is retained as a failed harness run, not reported green. The corrected run explicitly skips CUDA cases before discovery and does not compete with the learner. Existing prior frozen GPU equivalence proofs were not rerun alongside training.

The retained [pre-guard diagnostic script bytes](pnl-diagnostic-pre-gpu-guard.py.txt) match the script hash in both recorded diagnoses. They are an audit artifact, not the recommended entrypoint. Use the current guarded script, with CPU by default:

```powershell
python moloch/learning_actions/pnl_diagnostic.py --source-root ORIGINAL_FROZEN_SOURCE --source-run ORIGINAL_DEVELOPMENT_RUN --output NEW_DIAGNOSTIC_DIRECTORY --members 1 --steps 256 --device cpu
```

CPU and CUDA sampling need not produce the same trajectory. The exactness check compares instrumented and uninstrumented replay on the same backend. Reproducing the recorded CUDA numbers requires the original backend and an idle device; do not interrupt learning merely to repeat this diagnosis.

No active/frozen source files, production account worker, STOP, activation, risk cap or funded financial state were changed. These are already inspected development calendars; no independent-test source was opened. Full chronological three-venue history, calibrated lifecycle/queue/valuation behavior and independently demonstrated positive ROI remain unfinished. The [history/exposure audit](history-readiness-20261019.md) still applies.
