# Remaining SIG → Kalshi discovery, October16

This inventory is public metadata discovery, not a resolution-equivalence
certificate, current price or execution instruction. SIG party/race labels are
retained from the original catalog and normalized identity records; they do not
establish the complete settlement predicate.

The initial224-ticker universe left13SIG IDs unmapped. Actual original catalog
rows already contained Louisiana Senate `KXSENATELA-26NOV-D` and
`KXSENATELA-26NOV-R`, with explicit `election_year:2026`. A ticker filter wrongly
excluded the month suffix. Correcting that filter preserves all224 original
primary identities and adds SIG268/269, yielding226unique primary tickers.
These are original returned strings, not newly constructed ticker guesses.
Their historical metadata is not a current availability check.

| SIG IDs | Retained identity | Current discovery needed |
| --- | --- | --- |
|151,152|Democratic/Republican U.S.House winner|National chamber control; normalized state/office absent|
|153,154|Democratic/Republican U.S.Senate winner|National chamber control; normalized state/office absent|
|191|Independent Rhode Island Governor|Explicit independent predicate; not Republican/Democratic negation|
|276|Independent Montana Senate|Explicit independent predicate; candidate market alone is insufficient|
|280|Independent Nebraska Senate|Explicit independent predicate; candidate market alone is insufficient|
|303,304|Democratic/Republican Kansas Governor|Current official party-event metadata|
|372,373|Democratic/Republican New Hampshire Governor|Current official party-event metadata|

`sig-unmapped-identities.json` retains all13 initial identity records and the
original compressed source SHA256. It deliberately includes no derived live
quotes. The collector's primary loop should poll each unique selected ticker
once; a binary book supplies both YES and NO sides. Opposite-party contracts
must not substitute for a same-contract NO outcome when other parties can win.

Current canonical discovery will use only the existing centralized Kalshi
collector's local discovery endpoint once its owner confirms availability.
No independent authenticated client, credential access or fabricated current
catalog receipt has been created by this task. Close matches and unavailable
markets will remain explicit rather than forcing13successful mappings.
