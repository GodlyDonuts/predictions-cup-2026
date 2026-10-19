# Training reset — October 19, 2026

At 2026-10-19T09:42:11Z, the human-requested checkpoint reset removed all 36
previous run directories from the active local training path: 1,379 checkpoint
files and their optimizer/population/resume state, 158,351,287,149 bytes total
including other run artifacts. Verification found zero active run entries and
zero old learner queue launchers. No active Python learner was found.

Permanent deletion was rejected by automatic command approval review. The files
were therefore moved into a disabled local archive, not erased from disk. The
old learner launchers and source/resume locks were moved there too. Historical
published reports remain evidence of the old experiments, not usable checkpoints
for this fresh start. Raw/compiled market data and capture processes were
preserved. Production runtime and financial state were not changed.

The new raw API action path removes fixed decision frequency, lifetime decision
count, artificial strategy exposure caps and narrow price/quantity/expiry menus.
It supports policy-selected observations from all three venues and modeled SIG
single/batch/multi/all-or-none placement and cancellation. See
[current-training-action-profile.json](current-training-action-profile.json).

This reset does not establish full experiment readiness. Full competition-history
coverage across all three venues, calibrated complete API replay parity, and the
end-to-end daily-growth reward/evolution coordinator remain incomplete. No new
qualified profitable checkpoint exists. The new raw action policy is not the
currently deployed production policy.