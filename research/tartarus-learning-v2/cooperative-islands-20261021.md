# Cooperative island training — source implemented, GPU admission pending

The new training coordinator allows independent islands to advance without waiting for every island to finish a generation. CPU source workers can overlap their I/O; neural inference and optimizer operations share one parent GPU coordinator. Each model retains its own random state and on-policy update sequence. This is a scheduling change, not a new reward or a claim of profitable learning.

The complete per-island training/evaluation/replacement loop remains. Completed immutable champions can migrate only when their source, cases, calibration and evaluation context match and the migrant is better. Negative champions remain eligible. Migration follows the configured cadence, and the receiving model must earn its own new fitness. Checkpoints and old results remain intact.

There is one optional global independent test after selection, rather than one test per island. The hold-out reservation happens before its data is opened. Failure does not authorize trying another candidate on the same independent window. The live October 21–25 data remains excluded from fitting and development selection.

Verification: the combined CPU suite passed **89 tests in 97.529 seconds**, including serial/concurrent weights, optimizer state, random sequences, evaluation parity, spawned source workers, SQLite thread ownership, negative-champion migration and independent-test routing. An additional source-close-failure cancellation case passed separately. These checks establish scheduling mechanics; they do not measure RTX5070Ti capacity or returns.

The active production trainer remains run034/PID23524 on its original checked source. This change is **not GPU-admitted or active**. A measured two-policy GPU capacity check and an explicit checkpoint-preserving handoff are still required. Existing healthy training has not been stopped to make that diagnostic possible.

At 09:29 UTC, run034 had two completed members and was training the next one; the last logged total was731 optimizer updates/19,420 decisions. Member000's full GPU development selection had worst reward−0.142625 and worst marked ROI−10.254685%; member001's selection reported zero reward/zero marked ROI. Neither establishes a profitable independently tested model. A matched CPU evaluation of the completed656-update member000 is running with the same source, cases, seeds and actor-cost charge as the earlier parent/84-update comparison. It changes no weights and uses no independent data.
