# Default-off defensive cancellation

Source base: `2225b1efba5a16078cea875f5162f96742514d3a`. This is a runtime prototype; it changes no native financial event, rate limit, quote price, quantity, expiry, or accounting policy. `defensiveCancel` is an optional Control boolean whose missing value is **false**. Existing control/binary approval still governs activation.

The additional lane uses `ordinary_side_edge::assess_resting` (`1ca20ee0`): an opaque nomination requires an actual owned resting ordinary BUY, a genuine source and receipt strictly after its original POST ACK, and the same integer side-specific microprice/minimum-edge criterion used for entry. SELLs, reviewed/package orders, any opposite owned inventory, unknown/unavailable books and prior cancellation attempts are ineligible. Actual partial filled/remaining quantities are retained.

## Scheduling and sources

Mandatory cancellations and ready urgent owned exits run first. At most one optional cancellation replaces the pass's speculative preflight packet. It uses the existing cursor and budget, not a second HTTP client or limiter. Disabled mode does not scan or read.

`defensive_cancel_preflight` reuses four original sequential receipts: Tournament, Positions, complete open Orders, Account. The existing stream is serviced before each request; receipt clocks and bodies are untouched. The candidate is assessed again after the packet and its opaque current-context check runs before preparing DELETE authority. A changed, filled, covered, stale or gapped candidate defers without a cancel attempt. Control, STOP, source, durable and malformed-proof failures do not grant cancellation.

The permit is minted after facts/current-signal validation. Its expiry is the earlier of the original signal expiry and ten seconds from mint; more than the existing native write gap must remain. The exact actual order ID and `{qid}-cancel` intent remain bound by `ProductionAuthority::prepare_cancel` and the original SDK.

## Durable lifecycle and restart

The original dispatch log receives `cancel_attempt` before the SDK DELETE. Both cancellation lanes consult the new replay-derived `cancel_attempted(qid)` index; an unmatched attempt is never a retry ticket. Old duplicated cancellation rows remain readable. Neither an attempt nor ACK fabricates a terminal order, releases reservation, or changes native inventory. Exact owned-order/full-fill reconciliation continues; returned fills are applied before terminal accounting without a stream hook in that region.

The shared `cancel_owned` retains the original rejection reconciliation and fail-closed after-attempt error path. Cancel ACK only stores the genuine receipt and durable ACK. A successful DELETE can race with fills; only a subsequent complete official fill tape plus actual terminal order observation retires the remainder.

**Original source-checkpoint activation blocker (addressed by subsequent root integration):** this checkpoint does not infer no HTTP from `mayHaveExecuted`, an error string or missing queue binding. The parent is implementing DELETE-specific opaque SDK no-network evidence separately. Until that is integrated and checked, an SDK error after durable cancel attempt can still stop the process under the original policy; this feature must remain off. Pre-attempt expiry or an expected candidate change can defer without creating an attempt. The new index preserves uncertain attempts indefinitely unless a separately reviewed typed outcome path resolves them; eventual terminal expiry is still learned through ordinary reconciliation.

## Handoff and checks

APIs for parent integration are the existing optional Control field and graph's opaque `RestingDecision`; no SDK API change is required by this source checkpoint. Future typed no-network integration must bind the original DELETE spec, permit, attempt timestamp, actual order ID and SDK error to original SDK evidence, never rewrite the original attempt, and never make the qid retryable.

Authored checks: two dispatch replay tests and six worker tests using actual Session transitions. They cover OFF/no reads; a partial fill during four fact reads with unchanged receipt identity; improved/full-filled/gapped deferral; invalid/expired preflight with no packet; replayed unmatched cancellation; and ACK followed by a racing fill and actual terminal retirement. The resting-assessment dependency adds thirteen actual-Session tests. `git diff --check` passed locally. Rust execution and production behavior remain unverified here; parent owns ARM64 checks and promotion. No live service, controls or account endpoint was touched.

## Integrated source b112c6c1

An opaque owned-DELETE no-HTTP SDK proof now binds the original in-call failure, exact DELETE/permit/error, owned ID and actual held queue evidence. Before-admission evidence uses an explicitly named original-call-end capture cutoff; after admission it requires durable not_sent. Invoking the network marker permanently denies this proof. The optional consumer additionally checks account and Scope, appends source-only audit evidence and retains the original nonretryable attempt. Missing proof and unknown outcomes remain fatal; mandatory behavior is unchanged.

Scoped preflight GETs exclusively borrow the same Transport on one I/O thread while the original thread admits stream events to its sole Session. Only inner SDK errors use existing read recovery. Service errors join before failing; original receipts are recorded on the caller thread without a trailing callback. Known-order/fill/settlement reads and writes are excluded. No callback interrupts actual fill application or terminal accounting. Internal SDK pagination/backoff cannot be cancelled by this helper, so joining after a service error may wait for the original logical GET.

Actual ARM64 SDK231, library197 and worker95 checks pass. New helper seven checks and resting-assessment13 are included within these groups, not additional totals. The original worker94/95 missing receipt-directory fixture and library test-only Receipt Debug compile failure are preserved. Their corrections change tests only. Optimized release3ab6ef7c passes with unchanged compiled engine8acc; production PID525450 started with defensiveCancel:true; original journal restoration/fresh trading verification remain pending.
