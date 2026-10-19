# Separate observed-world snapshots from information delivery

The raw-action simulator now has an opt-in clock profile:
`original_observed_snapshot_world_separate_information_v1`.
It applies a recorded SIG book snapshot to hidden matching at its original
snapshot-receipt time. The recorded producer/laptop information clock is retained
for derived stream delivery. This is an observed-snapshot hypothesis, not a
reconstruction of every exchange update or calibrated market impact.

| Clock | Meaning in this profile |
|---|---|
| Original snapshot receipt | Earliest recorded observation used for the hidden matching book |
| Original native event receipt | Observed gap timing and an ordering bound |
| Original producer processing | Recorded processing bound, retained separately |
| Information availability | Original selected producer/laptop delivery clock; unchanged |
| Machine wall time | Training/evaluation compute time, separate from all market clocks |

Hidden book updates do not wake the actor or publish quote features. Orders can
fill against the updated observed world before a delayed quote reaches the actor;
private fill receipts retain their modeled notification path. Source gaps make
the hidden book unknown until another observed snapshot. A delayed information
event never reapplies its old book to matching or reexecutes a fill.

REST requests remain charged and delayed. Under this counterfactual read
hypothesis they snapshot the observed world at request time. Stream delivery uses
the particular original message, rather than accidentally copying a later hidden
book. A late older stream message, REST response or gap notice cannot replace a
newer known source state. Dropping a stale REST payload still completes its rate
budget, and cannot manufacture a new synchronization baseline.

Both source-world and information times must lie within the authorized
train/evolution/test calendar. An in-window world timestamp does not admit a row
whose information belongs to a later role. Burn-in cannot cross that role's lower
bound. Missing, inconsistent or future source clocks fail instead of being
silently retimed.

## Original metadata proof

The metadata compiler rechecks the original native byte chain and verifies that
every existing SIG row matches its original source before adding the event
receipt and processing timestamps. All market values and selected information
clocks remain unchanged. Existing external observations are reused through their
sealed parent manifest; the original source path and hash link remain recorded.

The actual retained-data extension verified 5,283 native book/gap rows. Its flow
extension has manifest SHA256
`3d6a963ccef25fd72be7209b07f1f5aa02e86b00c36945397a3c57f416d96e17`.
No network or account request was made. A CPU wiring check ran 64 neural decisions
in each of the train and evolution roles and verified 3,927 and 1,351 authorized
hidden-world rows. Its 14 evolution-case fills are simulated, not contest orders
or evidence of profitability. [Verification](snapshot-clock-real-source-verification-001.json).

## Learned-policy evaluation

The first phase-011 full-size candidate finished its training block at 93,437
decisions and 805 accepted updates. Its four original development cases returned
negative marked ROI, with the worst at -16.76777%. These are ten-minute blocks
under nominal/adverse hypotheses, not independent days or production P&L.

A separate CPU comparison freezes that same checkpoint
(`48fd834954457b1d64f2e82d91494b16dc1cfb3c4297ec772b9d615f23b2e52b`),
uses one common information tape and the same frozen phase-011 GPU compute
budget of 23 milliseconds per decision, and changes only the observed-world clock
profile. The original conflated-clock profile completed with worst marked ROI
-3.45483%; the separate-world profile completed at -3.45434%. The difference is
0.00049 percentage points, not a material learned edge. Both adverse cases
returned -1.3721075%, and all four cases recorded zero realized FIFO P&L. This
comparison is development sensitivity analysis and cannot unlock the final test.
[Complete frozen-policy comparison](frozen-policy-clock-sensitivity-001.json).

The focused set passes 115 checks. New cases cover hidden fills without quote
leakage, source/calendar fences, late-message ordering, cancellation after a
hidden partial fill, matching-book stability, metadata tamper rejection and
retained default behavior. Phase 011 remains on its original source-pinned
profile as a control; no live financial state or production worker was changed.

Continuous exchange-book reconstruction, original-owner exclusions, queue/impact
calibration, complete API/ALL collateral parity, the requested multi-day history
and independently demonstrated positive ROI remain incomplete.
