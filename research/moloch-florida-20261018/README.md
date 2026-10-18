# Florida Senate production review — October 18

**Actual new fill:** R44 bought10,0001044YES at0.835 at10:14:14.656UTC,
across four original official fill IDs.8,350 gross cost/zero fee reconciles cash
1,863.785 and owned FIFO93,872.925, with prior387fill rows retained (now391).
Attributed realized156.795 is unchanged: this is an entry, not earned profit.
[Independent source/accounting verification](../tartarus-20261017/checked-production-lane/r44-first-florida-fill-verification.json).

[Actual model activation and later runtime stop](activation-and-stop.json).

The existing R43 worker committed G11 generation `a28b77a2` without a restart,
preserving all eleven original reviews and the original financial limits.
The two additional reviews cover exchange1043 Democratic NO and1044 Republican
YES. Each permits at most10,000 principal; they share the same account bank and
Florida election exposure. They are not independent bets or20,000 extra cash.

Original forecast and official qualified-candidate HTML were captured directly
at09:05:55UTC and retained privately with hashes. The
[forecast](https://www.callthemap.com/senate/florida) displays Ashley Moody's
October17 candidate win point0.933. The
[official qualified list](https://dos.elections.myflorida.com/candidates/CanList.asp?CountyCode=ALL&OfficeGroup=ALL&Party=ALL&StatusCode=QUA&cantype=FED&elecID=20261103-GEN&sortby=OFF)
includes Moody REP, Angie Nixon DEM and Neil Gillespie NPA. Original SIG roots
separately establish the exact November3 party-winner contracts. A Moody win
is used as an operator-assumed subset of Republican YES and Democratic NO.
Third-party, write-in, replacement, resolver, refund and finality uncertainty
remain explicit. The forecast point is uncalibrated, not a payout guarantee.

The owner-selected exceptional allowance0.01, exceptional payout0 and margin
0.01 yield a model acquisition ceiling0.913670 before tick handling. These
allowances are assumptions, not estimated error bounds. The worker must obtain
current official account, contract and executable depth before placing an order.
Original publication clocks are unknown0; capture time does not become a new
publisher edition. Neither external prices nor dated SIG books are executable
cash or fresh trade permissions.

Reviews use the original official November4,17:00UTC settlement schedule with
`holdForActualSettlement`, bounded limit/maker execution choices, same-side
top-ups (at most16 entries inside the same principal cap), and FIFO profit-taking
with minimum0.005 per share. A quote or ACK is not a fill, and reaching an
accounting-profit floor does not establish whole-account ROI.

R43 accepted native sells18549 (78NO1002 at0.285) and18562 (10,000YES1067 at0.335),
then stopped09:14:08 on18577's local account-freshness rejection. No additional
fill or gain has been established. SDK history shows no18577 binding/admission,
no unknown acceptance, and a watermark earlier than its original attempt.
The original immutable error and archived STOP remain. A typed SDK no-HTTP
proof retired only the exact unadmitted intent; corrected R44 restarted09:42UTC
and is restoring the original financial history. The fix preserves20-second account checks and10-second book checks;
it does not turn uncertain writes into retries or erase old intents.

The installed R44 code also shares the existing selection cursor between passive
reserved-exit retries and new reviewed entries. Actual holding deadlines and
source revocations retain exit priority. No financial engine, fee, bank,
position basis or API-rate setting changes are involved.
