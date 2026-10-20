# Prospective independent test: October 21–25 UTC

The original October 16–20 window was exposed to development and cannot support an untouched-test claim. A new four-day calendar, October 21 00:00 through October 25 00:00 UTC, was explicitly reserved before its start. This does not rehabilitate the original test.

[Plan](prospective-holdout-20261021-25.json) and [actual reservation proof](prospective-holdout-20261021-25-proof.json) record the source pins. The real registry rejected an attempted training admission on the reserved dates without creating an exposure record.

`DataUse` now serializes reservations and admission writes using an operating-system lock. Reserved dates reject training, selection and inspected-development use. Independent testing requires the exact full reserved interval after it ends, and preserves existing one-shot finalist access. Four focused reservation tests and nineteen combined economics/population/reservation cases passed on CPU.

This is a local admission control, not proof against manual reads or other devices. Raw prospective capture is allowed; consuming it for development is not. The active frozen learner predates this change but uses October 19 development dates outside the reservation.

A new eligible lineage must train only before October 17 and evolve on October 17–21. October 19-trained development weights cannot become that lineage. Data extension, prospective SIG capture, complete daily reward evaluation and a qualified finalist remain outstanding. No independent test has been performed and no profitable model is established.
