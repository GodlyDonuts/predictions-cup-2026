> Current deployment, results and unfinished research: [October 17 handoff](MOLOCH_RESEARCH_HANDOFF_20261017.md). The dated sections below preserve historical evidence and are not the current deployment specification.

# Paper6 exhaustive payoff floor candidate

`moloch/native/src/semantic_floor.rs` is a pure offline solver and certificate
checker. It is deliberately separate from `semantic_states::Compiled` and
`probabilistic_portfolio::Plan`: a reviewed finite probabilistic scenario list,
positive expected wealth, or downside-floor improvement cannot become a
guaranteed marginal gain merely by renaming it.

`ExhaustivePayoffCertificate::verify` requires two independently configured
approval sets: exact certificate bytes and original reviewed completeness-proof
bytes. The latter must explicitly bind every official contract-root hash, exact payoff-matrix SHA, state
ID, proven fee bound and exception family, assert a disjoint exhaustive partition, and contain no
unresolved obligation. Each ordinary, fusion, independent, replacement, runoff,
not-applicable, cancelled and other-admissible-outcome family needs actual
covered states or an independently approved source-backed impossibility proof.
Zero modeled probability and missing rows never establish impossibility.
All original sources (32 MiB aggregate bound), receipt endpoints, scope, rule hashes and original clocks
are checked; source expiry cannot be renewed by loading it again.

This is a verifier of **explicit independently reviewed proof authority**, not
an automated natural-language theorem prover. Approval sets must come from the
operator's checked interpretation of original official rules. The module does
not create or approve either set. Synthetic unit-test proofs are not real venue
certificates and must never be installed in production.

## Operable API and economics

`solve(Some(&certificate), &Context, &[Candidate])` evaluates at most 4096 bundles,
each with1–10BUYlegs. It returns `Uncertified`, `NoOpportunity` or
`GuaranteedFloor(AtomicProposal)`. The latter contains the entire certified-state
net vector and its strictly positive minimum, full gross limit funding, observed
multi-level executable cost, maximum nonrefundable fees, original receipt hashes (including fresh unchanged official nodes),
scope/head/run bindings and a short original expiry. Its execution mode is always
`allOrNone`. There is no sequential dispatch fallback, financial reservation,
HTTP call, ACK accounting or fill invention.

YES buys sweep actual ask depth; NO buys sweep complementary YESbid depth. Every
level must have valid positive quantity, tick prices and strict ordering. Depth
below the actual fixed limit is never counted at a better price. Partial depth
is not an opportunity. Gross limit notional plus the reviewed fee bound must fit
the caller's genuinely reconciled unencumbered cash, even if observed depth costs
less. The original fresh tournament cash receipt is independently scoped and
caps that amount; it does not itself prove existing reservations, outstanding
borrowed principal or native ownership. The existing financial authority must
derive the unencumbered amount and match the supplied current native head.

Each normal state uses fixed-limit acquisition cost as a conservative maximum.
A principal refund cancels that leg's original acquisition debit, regardless of
its eventual execution price. It is not a dollar-per-share payout and does not
refund fees. Thus an admissible all-principal-refund state correctly prevents a
strictly positive guaranteed profit. Fees, missing exception coverage, old books,
unfunded bundles or merely positive expectation cannot bypass this conclusion.

Initial scope is flat BUY packages only. There is no claim to value retained
inventory, use SELL receipts as funding, optimize credit, or model dynamic repair.
Existing SDK typed allOrNone execution still requires fresh full account/rules/
book preflight, a genuine bank-bound reservation, stable durable intent, physical
authority, exact unchanged limits and unknown-acceptance protection. This module
is registered only for isolated native checks and is not wired to physical execution; parent integration remains required.
Current SDK/runtime leg bounds may be more conservative than the venue's10leg
schema; this proposal does not widen those guards.

## Actual retained SIG evidence and unproven obligations

Read-only local original node receipts inspected for this task:

| Market / exchange / contract | Receipt SHA256 | Original receivedUTC |
|---|---|---|
|Iowa 260 / 949 / 261|`da6b42629c556908490cc54940b0b00d894f180db825c7c8c0316e46f83ca0d0`|2026-10-15T13:31:27.356Z|
|North Carolina 277 / 966 / 278|`d26657b5578452c5ad819693ab6c55e1ed0ddc1dff7b25a52fa4d1a30ccf1111`|2026-10-16T05:54:07.744Z|
|Kansas 356 / 1045 / 357|`9daa5d5458a8417634b2f3fd0fc4075c42f26b52542c6e40cc556684446c595f`|2026-10-15T19:08:55.461Z|

Their `/data/root/contract_details` fields establish Election Outcome,
General-stage, Party Winner, Democratic Party, exact raceId/state and electionDate
2026-11-03. `/data/root/settlement_date` is scheduled 2026-11-04T17:00Z. These are
different races, not an exhaustive one-of-three outcome set. The retained nodes
contain no description/children/metadata resolving all exceptional outcomes.
They do not prove other-party impossibility, fusion/replacement behavior,
runoff/delayed/cancelled handling, sponsor overrides, exact principal-refund
rules or a complete mutually exclusive cross-contract partition.

A same-binary YES/NO relation is a more plausible structural proof target than
unrelated election races, but actual venue settlement/refund/fee/auto-netting
semantics must still cover every admissible state. In particular a full refund
of original principal destroys a strict positive terminal-profit floor even
when ordinary YES+NO payouts total one. No actual exhaustive certificate was
created from these retained sources, and no current profitable guarantee is
claimed. The correct current output without additional reviewed official
completeness evidence is `Uncertified`.

## Isolated ARM64 validation

The initial13focused tests passed on a private copy of the previously checked
unified source. No Rust compiler fix was needed. Separate follow-ups make
`AtomicProposal` opaque outside this module (read-only getters, noDeserialize)
and cap combined solver work. Their final exact-source rerun is recorded in the
parent handoff; this does not validate any real venue completeness certificate.
No production bank, service, account client, permission or runtime was changed.
