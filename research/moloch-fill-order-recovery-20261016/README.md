# Actual deployment and subsequent BUY-tie failure — October16,19:28UTC

The recovery deployed at18:57:57 as PID534982 and restored the complete original
history. At19:17 it genuinely traded and automatically booked the earlier447NO
SELL loss of2.160000, with206 cumulative fill rows. Its first new official order
bought2,094Oregon NO for1,949.320000, but the BUY mixed-price timestamp guard
stopped it at19:17:27 before booking those five fills. The original known ACK,
closed order, complete tape and flat prior inventory are verified; SDK unknowns
are zero. No fill was manually appended. See deployment/production-summary.json
for the actual rollout and deployment/second-fill-order-failure.json for the
later failure. The historical checked build/model evidence below remains valid;
it does not establish current uptime or profitability. The second recovery is
in progress and requires an explicit tied-BUY accounting convention.

# Recovery of legitimate simultaneous SELL fills

Production525450 stopped at18:31:17 UTC, October16, with
ACCOUNT_FILL_ORDER_AMBIGUOUS. This was an actual parser failure, not an operator
pause. The same known physical SELL43438003/logical8794 executed447NO shares:
432 at.100 and15 at.105, both stamped18:31:04.464. The complete original tape,
closed order, original ACK and native447NO@.105 prove44.775 proceeds and46.935
FIFO cost. The missing realized effect is **−2.160000**, not a profitable exit.
No fill was manually appended. Before recovery these IDs were absent and native
filled quantity remained zero. Other resting orders may still fill while stopped.

The repair preserves each original price, quantity, ID, receipt and timestamp.
Within a mixed-price SELL timestamp it keeps original stable tape order and
explicitly records that finer chronology was not inferred. Total proceeds and
consumed FIFO basis are invariant to this tie order; partial-sale residual lots
are covered by a native Wallet regression. Different-price BUY timestamp ties
still fail because residual cost-basis ordering can matter. Coverage, duplicates,
totals, order scope and original terminal proof remain required. This does not
solve historical cross-order late-observation FIFO.

Checked source `ec8b0fef24ec6b43960784c05d40b7862a7643e3` also contains the previously checked negative-only early
ordinary-entry screen. Exactly3/251 build inputs changed fromb112c6c1;248,
including financial native and SDK bytes, remain exact. Actual ARM64 library208
and worker97 checks, optimized release and compiled-native probe passed. Four
new fill tests are subsets of208; the unchanged SDK was not rerun. Binary
`40fbaa4731be72c735b6bfc6bff86423cd833e57e20ad80640935d585be8a8d6`, native identity `8acc461bc02fb9236100058fe464b203d5da005375632af05c176223c7246754`. Release took232.19 seconds. The first build
helper invocation lacked cargo in PATH and failed before compilation; its original
log is retained alongside the corrected successful run.

The recovery operator has stage/promote only. It requires exact original failure
STOP/status/tape bytes and a named root recovery review, absent old process,
exclusive existing capture lock, quiet SDK with zero unknowns, unchanged bank,
dispatch and twelve bridges, checked source/binary and an independent root
witness. It archives only that diagnosed failure after rechecking the boundary.
It cannot reuse the earlier controlled-stop witness or clear an unrelated STOP.
Nine pure operator guards pass; all actual frozen log/source/model/failure guards
also pass offline. The first private freeze refused the old reviewer identity;
the final guard explicitly requires the current named execution-owner reviewer.
No financial assumptions were changed by that correction.

The same planned recovery includes two explicitly reviewed terminal models:
refreshed NC277 Democratic YES and new flat Oregon288 Republican NO. Each has
a2,000 principal cap. Genuine current CalltheMap point assumptions are.919 and
.995; both use the existing operator3% exceptional allowance, zero exceptional
payout and.01 margin. These allowances are **subjective stress assumptions,
not calibrated error bounds or arbitrage**. Tick entry ceilings are.880 and.955.
The NC historical quote did not qualify. Oregon's historical.925 ask requires
fresh exact depth/account/rule checks before any order. Kalshi is corroborating
predictive context, not the probability source or an executable hedge.

Actual Rust typed/current review checks and Settings.load verify both originals,
contract roots, scheduled settlement objective and all four review entries; DE/MA
remain unchanged. The probe uses checked248b's identical fundamental parser,
whose source hash is unchanged inec8b. It does not establish forecast calibration.
NC retains its original node clock/24h TTL and expires23:45:21 today. Oregon's
static node reference has an explicitly reviewed48h TTL, endingOct17 12:37:03;
its original clock and mandatory fresh physical root check remain unchanged.
Unknown official ballot publication time is explicitly0, not a claimed date.
Positions hold through actual settlement, including delay; expiry blocks new
acquisition and does not invent an intraday cash forecast or force liquidation.

At publication this is checked recovery/staging evidence, **not fresh production
or profitability proof**. The original whole bank, attributed positions, full
95,579.915000 capital mandate, twelve bridges and sole account queue remain.
See build/validation.json, deployment/owner-recovery-review.json and
forecast/validation.json. Original source bodies remain private and hash-bound.
