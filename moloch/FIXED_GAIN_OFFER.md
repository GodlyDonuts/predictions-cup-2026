# Stable covered profit offers

Optional `inventoryExitMode: "fixedGain"` sets a covered passive exit to the lowest legal tick that covers the most expensive remaining lot, the requested net gain, and conservative one-share entry/exit fees. It holds that price when the external ask changes, retaining queue position instead of continually following the ask. The normal mode and all historical/default policies remain unchanged.

The offer may rest above the current ask; it is not assumed filled or profitable merely because it exists. Unreachable targets at or above one are skipped. A positive finite maximum inventory age is mandatory, and the existing age stop can still dispatch a losing depth-supported exit. Funding, share pledges, attribution, latency, write pacing and original quote-window reconciliation remain enforced. Flat-only policy changes can select the new mode without resetting bank history.

Synthetic tests show a covered one-tick offer can intercept eligible opposing flow while a farther join offer does not. They also check YES/NO complements, conservative fee margins, price retention, the legal price ceiling, mandatory maximum age and a forced losing exit. All134 core tests pass.

The complete already-observed cycle42–46 yields +0.125 under both the fixed offer and the prior one-tick taker candidate, with no inventory or unresolved quotes. That is not a demonstrated improvement or untouched validation. Evidence: `data/moloch-fixed-gain-cycle-20261014.json`. Original live bank cash and the actual completed zero-return cycle remain unchanged.

The existing exchange1077 forward phase53 ended with60 receipts and zero new fills. Phase54 drains its two remaining funded quotes per bank on the same scope. The fixed-offer mode was not enabled during that phase. Entry result: `data/moloch-recent-flow-entry-result-20261014-53.json`. A separately registered fresh trial is required before promoting the mode.
