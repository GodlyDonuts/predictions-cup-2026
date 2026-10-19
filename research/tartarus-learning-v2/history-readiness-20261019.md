# Full-history ML feasibility audit

**The requested chronological experiment is not ready.** At the fixed audit cutoff2026-10-19T23:00:05Z, the inspected compiled sources do not reach the required training partition. The last four days also overlap recorded development/optimization exposure. More optimization on the current short tape cannot supply missing original history or establish an untouched independent test. GPU017 continues its separately labeled development experiment; the checked least-loss/faster-acting018 successor remains queued.

## Requested calendar at this cutoff

| Role | Required calendar, UTC |
| --- | --- |
| Training | Competition start, still requiring official provenance, through October11,23:00:05 |
| Evolution | October11,23:00:05 through October15,23:00:05 |
| Independent test | October15,23:00:05 through October19,23:00:05 |

All intervals are half-open; a boundary belongs to the following role.

The metadata audited here has these **outer clock extents**. They are not continuous-coverage claims, unique independent observations or equally fresh observations across every market.

| Venue | First included compiled clock | Last included compiled clock |
| --- | --- | --- |
| SIG | October15,08:09 UTC | October19,14:09 UTC |
| Kalshi | October16,17:38 UTC | October19,14:09 UTC |
| Polymarket | October19,04:16 UTC | October19,14:09 UTC |

Earlier raw-file receipt inventories include additional external captures, such as Polymarket from October18,22:35UTC, but do not establish a verified continuous three-venue corpus. The current longer cohort also retains its documented84.413-second SIG gap. Relocated copies, overlapping compiled cohorts and replay repetitions cannot add new historical days.

The local exposure registry contains139 overlapping declarations within the requested four-day test:67training,67selection and5inspected-development records. Their overlapping intervals have a union of49.661hours; that is declared exposure, not continuous venue coverage or139 independent experiments. Some declarations conservatively mark inspected development and need not imply parameter training at every timestamp. They still prevent a clean independent-test claim on that same calendar. No final test was opened or scored by this audit. Previously protected smaller cohorts remain unchanged and are not relabeled.

[Exact calendar, input hashes, extents and exposure union](history-readiness-20261019.json).

## Undated archives missed by the earlier filename filter

Read-only SSH inspection found seven `crossvenue-observer-trade-trace-v2` through `v8` directories whose raw filenames have no date. The seven trace-start headers and terminal seals span October16,16:24:29.852–23:55:03.388UTC. All14 compressed prefixes, totaling3,918,326,303bytes, match their declared seal hashes. Six seals declare completion; v7does not. All seven producer seals have `wholeWitnessVerified=false` and `historyPrefixVerified=false`.

These checks establish compressed-prefix integrity and header/seal metadata, not event-by-event source-clock semantics, complete market coverage or independent execution calibration. Existing separate verified research artifacts remain their own evidence; a producer's false witness flag alone does not disprove a later external verification. Different archives still need their own verified source/semantic replay before admission. These headers do not extend the inspected data back to October11. Only the trace header was decoded in the final prefix audit; compressed sample/body events were hashed without decoding their prices or outcomes. [Archive metadata and exact prefix hashes](undated-trace-header-inventory-20261019.json).

## Current source and recovery limits

At23:02:53UTC, the existing VPS `moloch-live.service` reportedMainPID0, inactive/dead, result`exec-condition`. The live execution STOP and shared NETWORK_DISABLED markers were present. The retained local original-SIG capture also ended with`producer_stopped_or_changed`, with both capture/provider completeness false. No service, STOP, credential, account client or financial state was changed. [Fresh service/control observation](sig-source-service-metadata-20261019.json).

The saved official SIG OpenAPI SHA256`fbdf23e54d317392f9fb4a659b9511a01fad065e7a384f28091ef04894a732c9` describes `/exchanges/{id}/price-history` as sparse OHLCV candles computed from trades. Empty buckets are omitted. Those candles are useful potential historical information, but do not supply original order-book depth or queue events. They must not be represented as observed historical books with invented quantities. The saved trade endpoint similarly does not, by itself, reconstruct the full historical book. This is a saved-schema observation, not a fresh authenticated account request. Official API: <https://sig.thesuper.market/api/v1/docs>.

Full qualification needs an earlier original corpus or organizer/provider historical export, verified three-venue chronology and execution/valuation coverage, and a predeclared genuinely untouched four-day test at a later fixed cutoff. This audit does not choose or silently move that cutoff, retire old protected calendars, or make a partial development run eligible. Rebuilding fresh SIG observations also requires an authorized data path that respects the existing STOP/network controls and sole official account worker.

## Reproducible metadata-only check

`python -m moloch.learning_actions.history_readiness --clock-inventory research/tartarus-learning-v2/compiled-history-clock-inventory-20261019.json --manifest PATH_TO_SEALED_DATASET_MANIFEST --cutoff-ms 1792450805000 --output NEW_REPORT.json`

The tool reads existing clock inventories, manifest/seal metadata and local exposure records. It opens no observation stream, exports no prices/returns, performs no account requests and never grants full-history readiness from outer extents or source flags. Seven focused tests passed, including half-open boundaries, overlapping exposure unions, missing registry uncertainty, invalid clocks/hashes, untrusted price-field exclusion, manifest seals and read-only registry inspection. It is a standalone audit; current/pending learner sources and production controls are unchanged.
