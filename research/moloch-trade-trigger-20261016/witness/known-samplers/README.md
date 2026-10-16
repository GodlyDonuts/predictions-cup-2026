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
| `2fa9b142b672f4a9076e185ac91be0b288945b0e1bba6afb0797594ed6678529` | Current reviewed `moloch/external-data/crossvenue_observer.py` from `063fdd07` |

The current version's Git LF bytes are63,523bytes. For repository checkout
compatibility only, its exact fully CRLF encoding
`4a2d585d9b2ce81add31cc2a6a8ba99fb6df6bcd019dbf167535e4e1910f3471`
may supply those same reviewed implementation bytes. That hash is **not** an
approved capture-producer hash. Local code hashes are checked against the
selected version's own finite byte allowlist before direct `compile`/`exec`.
There is no arbitrary newline-normalization admission, manifest-selected code,
cached `.pyc`, or fallback to a newer sampler. Future changes to the current
source fail closed until separately versioned and reviewed.

The verifier's result schema, completeness/authority limits, source membership,
input ordering, output equality and terminal-seal checks are unchanged. The
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
