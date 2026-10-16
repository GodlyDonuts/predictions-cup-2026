# Fixed reviewed sampler versions

`crossvenue_observer_200ab.py` is the exact Git LF blob from
`db87606d2166a4e3a7d9e68aae2f980e1bcd9490:moloch/external-data/crossvenue_observer.py`:
61,865bytes, SHA256
`200abfd0701f56f43183b1b7b60cd1cdf11525f0fb9daea72e9bf6f08d90bc40`.
It is retained unchanged so historical traces keep their original sampling and
sequence-only book deduplication. It is not a second collector or active process.

`trace_replay.known_sampler(producer_sha)` selects only these fixed repository
implementations. A manifest's producer path is pinned **input evidence**, never
an executable module path:

| Original capture producer SHA256 | Fixed reviewed replay implementation |
| --- | --- |
| `200abfd0701f56f43183b1b7b60cd1cdf11525f0fb9daea72e9bf6f08d90bc40` | Historical file in this directory |
| `1f63cf06d05b68e46d2febe57607125d9eb95d6e87db3f4d29b401d39d8e700b` | Same historical file; exact known CRLF encoding |
| `2fa9b142b672f4a9076e185ac91be0b288945b0e1bba6afb0797594ed6678529` | Exact immutable `crossvenue_observer_2fa9.py`; original V6 ordering |
| `3d3b5d43a6c79293d28b424fc4ab31626267acdd1c1b71918d9cdefd94824cd7` | Exact immutable `crossvenue_observer_pretrigger_v2.py`; prospective ordering v2 |

The retained2fa version's Git LF bytes are63,523bytes. For repository checkout
compatibility only, its exact fully CRLF encoding
`4a2d585d9b2ce81add31cc2a6a8ba99fb6df6bcd019dbf167535e4e1910f3471`
may supply those same reviewed implementation bytes. That hash is **not** an
approved capture-producer hash. Local code hashes are checked against the
selected version's own finite byte allowlist before direct `compile`/`exec`.
There is no arbitrary newline-normalization admission, manifest-selected code,
cached `.pyc`, or fallback to a newer sampler. Future changes to the current
source fail closed until separately versioned and reviewed.

For the original producer versions, result schema, completeness/authority limits,
source membership, strict input ordering, output equality and terminal-seal checks
remain unchanged. The
analysis adapter changes only its fixed verifier LF hash. It does not approve
models or reinterpret prior cohorts. No actual cohort/outcomes or network was
read for this task.

Synthetic checks:24 witness tests (the original19 plus5 version/source-selection
tests),14 analysis-adapter tests and22 parent-analysis tests all pass. The new
end-to-end fixture records the same engine-sequence/new-read-time input under
historical LF, historical CRLF and repaired samplers: historical replay emits its
original single feature row; the repaired version emits a later feature and
horizon label as well. Source substitution and equal-length mutation of either
fixed implementation are refused. Default historical fixture behavior remains.


## Prospective pretrigger consumption version2

The current producer and its immutable new snapshot have exact Git LF SHA256
`3d3b5d43a6c79293d28b424fc4ab31626267acdd1c1b71918d9cdefd94824cd7`.
Its exact local fully CRLF encoding
`83733121085c962c32dadeb12a08eba6dbf8562e1e9e188d91607176534017b5`
is accepted only as a local reviewed implementation variant, NOT as an approved
capture producer. All original200ab/2fa implementations remain separately pinned.
The2fa bytes were extracted unchanged from their original Git blob; the loader no
longer relies on the mutable current producer for historical V6 verification.

The underlying capture container/version1 and economic sampling policy SHA256
`031edcb2d4d6ca144e89d86b467e6b879113f1e172c3ef19457f2b577996717b`
remain unchanged. Only the new producer's run_start carries the exact source-bound
`sigDrainPolicy.version=2`: after a complete original WS HTTP response is pinned
and parsed, consume bounded already-readable original SIG journal rows BEFORE
calling the trade sampler. Read the decision clock only AFTER that consumption.
Original cache request/response/body/export, trade execution/receipt/availability,
and SIG source/socket receipt clocks stay original. Nothing is backdated.

One drain reads at most4MiB and rotates at most once. Every returned complete row
is processed in order; it never breaks a Tail.read generator and drops its remaining
rows. A whole-line boundary is not EOF: old-segment bytes must be fully consumed
before rotation. A newly opened tail is closed on failure, while the caller retains
its original descriptor for an incomplete failure seal. If more backlog remains,
the same unchanged age gates may still reject the trade. No deadline or yield is
promised by this bounded scheduling improvement.

The new replay branch permits original sig_row/rotation ONLY while a COMPLETE WS
body is pending and has NOT been applied, with matching trade-mode marker, read
budget and rotation bounds. Pre-body or post-apply interleaving fails. Old2fa and
200ab still reject all such interleaving. Prospective deployment requires a new
producer pin and separate cohort; V6 skipped IDs, originals, protocol and old output
are not rescued or relabeled. Unchanged book versions, old source timestamps,
gaps, malformed source, old external trades and the2s/3s/20s gates still behave as
before. Missing price features remain missing and no financial authority is added.

Focused synthetic checks at handoff:42 observer tests,17 trade-trigger tests,
16 original-input trace tests and31 replay tests,106 distinct total, all PASS.
The31 include all prior24 plus7 new positive/adversarial replay cases. The six new
observer cases cover post-body ordering, actual later clocks/no expired-trade
rescue, malformed/body-mode bypass, whole-line unread-tail preservation, finite
read bounds and descriptor cleanup after rotated callback failure. Existing old
producer1/3-output-row fixtures and default book-mode golden bytes still pass.
No actual V6 outcomes, network calls, deployment or production actions occurred.

The old coverage/analysis artifacts remain tied to their original producer,
verifier and enrollment hashes. A future analysis wrapper needs an explicitly
reviewed exact new verifier pin; changing this source does not qualify a model,
prove a complete source witness or authorize execution.
