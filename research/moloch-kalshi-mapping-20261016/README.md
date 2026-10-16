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

## Current returned metadata

After the owner confirmed availability,16public metadata requests used only the
existing centralized collector at localhost17841. Original full receipts are
losslessly gzip-retained in `receipts/`; `source-inventory.json` records exact
request/receive clocks, official paths and compressed/original/bodySHA256.
The original bodyText hash is checked independently by the universe loader.
No independent authenticated client or credential access was used.

| SIG | Actual returned ticker | Scope of result |
| --- | --- | --- |
|151,152|CONTROLH-2026-D / CONTROLH-2026-R|2026House control party rules|
|153,154|CONTROLS-2026-D / CONTROLS-2026-R|2026U.S.Senate control party rules|
|303,304|GOVPARTYKS-27-D / GOVPARTYKS-27-R|Rules explicitly2026election; event subtitle says2027|
|372,373|GOVPARTYNH-28-D / GOVPARTYNH-28-R|Rules and subtitle explicitly2026 despite28ticker suffix|
|191|GOVPARTYRI-26-KBLO|KenBlock-specific, not all Independent-party winners; candidate analog only; no pricing authority|
|276|SENATEMT-26-IND|SethBodnar-specific, not all Independent-party winners; candidate analog only; no pricing authority|
|280|SENATENE-26-DOSB|DanOsborn-specific, not all Independent-party winners; candidate analog only; no pricing authority|

`SENATE` and `KXSENATE` series/events/markets returned200empty; the actual
returned series catalog identified `CONTROLS`, whose event response supplied
the2026control markets. All requested event pages closed with empty cursors;
this is bounded observed coverage, not proof of every Kalshi market's absence.

The dedicated `supplement.json` adds8same-party conditional discovery mappings.
It preserves all226existing primary ticker identities. Total234unique primary
tickers map234of237SIGmarkets, leaving191/276/280 explicitly unmapped. At20read
starts/second,234reads imply an ideal11.7second sweep, excluding network/pacing
overhead and metadata refreshes. Public metadata discovery consumed the same
collector budget as its normal polling.

For the misleading27/28suffixes the loader uses an exact source-backed whitelist,
not a broader ticker-year guess. Exported `metadataIdentitySha256(market)` hashes
the JSONarray of original `[ticker,event_ticker,title,subtitle,rules_primary,
rules_secondary]`, using null for missing fields. Each supplement mapping pins
this hash and `verifiedElectionYear:2026`. A collector may accept such a ticker
only when its freshly returned metadata matches that exact identity. Actual2028
rules remain rejected. Price/volume updates alone do not alter the identity hash.

Special-election status is null when not explicitly supplied; this source does
not certify special/regular equivalence. Full cross-venue settlement treatment
(certification versus inauguration/seating, replacement, cancellation/refund,
independent affiliation and accelerated determination) remains unverified for
every mapping. Candidate analogs are retained separately and never silently
substitute for the generic Independent-party contracts.

## Separate candidate-reference coverage

`candidateAnalogMarkets` contains the three exact returned KenBlock, SethBodnar
and DanOsborn market identities, each pinned to original receipt/body/metadata
hashes and the original event's explicit `In 2026` field. The normal `markets`
array remains234same-party primary discovery mappings. The three independent
SIGmarkets remain in `unmappedSigMarketIds`; analog observation does not change
that qualification. Each analog has `mappingKind:candidate_reference_analog`,
`resolutionEquivalenceVerified:false`, `pricingAuthority:false` and an explicit
narrower-candidate-rule caveat. A collector may collect these separately after
checking the exact live metadata identity; they must not enter a same-party
pricing/settlement pipeline.

Summary separates234direct mapped SIGidentities,3analog-covered identities and
237total identities with some reference. The combined237unique tickers require
an ideal11.85seconds at20read starts/second, before overhead. Neither that
arithmetic nor all237reference coverage establishes237equivalent contracts.
