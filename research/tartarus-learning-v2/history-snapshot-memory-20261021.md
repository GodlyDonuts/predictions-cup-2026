# Automatic history snapshot memory correction — October 21

At 12:43 UTC, population036 had stopped with a CUDA allocation error. Its
four-update recovery checkpoint and all four parent models remain available;
no generation completed and no profitability result was obtained. The failed
input transfer requested 26.21 GiB on the laptop's 12 GiB GPU.

The V3 automatic service paid for the correct Kalshi/Polymarket polls, but every
repeated latest response was retained as a separate neural input until the next
decision. An allowed uninterruptible WAIT spanning hours could therefore create
millions of duplicate snapshot entities. This was an observation delivery bug,
not evidence that the neural network itself required more GPU memory.

The explicit V4 profile `model_pausable_history_snapshot_service_v4` keeps one
latest automatic receipt per exact venue/endpoint/source/market/side between
decisions. All original simulated requests, rate admissions, receipts, source
clocks and audit counters remain. Actor-requested historical pages remain
complete. A newer receipt arriving during inference survives consumption of the
older observed sequence. The model can still pause collection and choose long
waits; no action or decision-count cap was added. Frozen V3 runs retain V3 behavior.

Fifty focused checks passed, including a 60-second uninterruptible wait with more
than 2,500 V3 receipts, exactly identical V3/V4 paid schedules and finances, and
only two pending V4 snapshot entities for the two fixture sources. Explicit pages,
inference-time arrivals and checkpoint profile binding also passed. These are
delivery checks, not ROI evidence or a whole-run throughput benchmark.

The successor must preserve population036's four new accepted updates and the
other three founders, use fresh optimizer state/trajectories/scores, and keep the
same reward and calendar boundaries. Independent testing is still protected and
no learned model has qualified for production. The live production worker remains
separate from this research correction.
