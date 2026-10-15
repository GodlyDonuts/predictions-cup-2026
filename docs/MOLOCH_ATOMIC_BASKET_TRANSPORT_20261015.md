# Strict all-or-none package transport

Task [#127](https://github.com/GodlyDonuts/predictions-cup-2026/issues/127), session `Viserion/all-or-none-feed-20261015`, based on main`9fdd02e5`. This SDK extension cannot activate a strategy or reserve a native bank. The current live Authority explicitly rejects every POST other than `/orders`; `/orders/multi-leg` remains unavailable to production until the execution owner implements and checks separate package authority, reservations and complete-fill reconciliation.

## Venue contract

The audit uses the official saved OpenAPI`runtime/moloch/api-spec-ioc-20261014.json`, SHA256`6b5ff9a661c06f1a2d1dda131bf870a70f4f79699ac9937375a278861e1f81bf`, at `/paths/~1orders~1multi-leg/post` and `/components/schemas/MultiLegOrderData`. Public reference: [official API docs](https://sig.thesuper.market/api/v1/docs). A fresh production schema/availability check belongs to the sole execution worker; this task made no account requests.

The important distinction is execution mode:

| Mode | Venue guarantee | Remaining risk |
|---|---|---|
| Default multi-leg | Transactional order admission; legs may partly fill or rest | Unmatched inventory and resting liabilities |
| `executionMode:"allOrNone"` | Every leg immediately fills its entire quantity within its limit in one atomic engine execution, or none executes/rests | Settlement, adverse repricing, funding, account attribution and uncertain transport responses |

All-or-none is engine-routed only. The SDK forces the exact camelCase value`allOrNone`; the early proposed snake_case field is not supported. `relationshipConstraint` is optional, so an empty ALL relationship registry does not prevent an otherwise authorized full-fill basket. Supplying a UUID adds a preflight price/membership check, which the docs explicitly say is not held under the execution lock. It does not itself prove settlement equivalence.

`400 VALIDATION_ERROR` with exact `error.details.rejectionReasonCode:"AllOrNoneNotFilled"` and a valid zero-based `leg` means no leg executed/rested. A killed request retains its idempotent result; retrying that same key cannot convert it into a new opportunity.

## Request interface and bounds

`Transport::place_all_or_none(multi_leg::Basket, Permit)` reuses the existing client, Authority and persistent account admission queue. It adds no concurrent client, credentials, native ledger mutation or retry loop.

The first implementation accepts **two or three distinct exchanges, BUY only**, each with an explicit YES/NO side, positive integer quantity and0.005-tick limit in0.005–0.995. All wire legs carry the package's exact configured tournament UUID. Market-price sentinels, omitted prices, unbounded market orders, default execution mode, duplicate exchanges and cross-tournament legs are impossible through this interface. Buy-only handles acquisition of general binary payoff packages while avoiding an unreviewed unbacked-SELL→complement canonicalization path; SELL support requires its own ownership/accounting design.

Maximum cash reservation is the checked sum of **full gross per-leg limit×quantity**. It must fit both `Basket.maximumCost` and the existing `Permit.maximumCost`. `Permit.maximumQuantity` bounds the **aggregate submitted shares** in this method, not each leg separately. ALL advances, opposite-share redemption credits and later proceeds never reduce this initial gross reservation. The native execution owner must still ensure the reservation actually exists and all physical/account checks remain fresh.

The ordered legs, side/action, exact wire limits, explicit tournament, idempotency key, optional relationship UUID and fixed execution mode form the request identity. There is one package key for the logical basket, and one package run/intent binding through the same queue. The caller cannot change payload or issue a replacement after an uncertain response.

## Evidence before interpretation

Before network, after the first exact Authority check, an immutable0600 private request file retains the complete RequestSpec, Basket economics, package key and run/intent identity; its hash is returned as `RequestEvidence`. This ensures even a connection loss leaves the original payload available for reconciliation. It is evidence of an attempted request, not authorization or a fill.

For this endpoint only, `perform` persists bounded raw response bytes before JSON parsing. A0600 `.body` file preserves the original byte sequence; a0600 metadata file records status, source clocks, complete-body flag, exact request and hashes. Incomplete/oversized bodies retain at most the existing8MiB-plus-one-byte cap and cannot be interpreted as definitive errors. No authentication headers or key material are stored. Existing non-basket response behavior remains unchanged.

`last_basket_response_evidence()` provides private evidence paths/hashes for diagnostics. These files contain account/order data and must not be committed or copied into public GitHub reports. Public reports should expose aggregate status/hash references only.

## Outcomes and reconciliation

| SDK outcome | Required evidence | Accounting action |
|---|---|---|
| `FullFillReported` | Exact ordered result count/index; distinct known order IDs; pinned exchange/side/BUY/limit; original full quantity; zero remainder; closed status; traded magnitude equals requested quantity; bounded reported notional; valid nullable ALL economics | Fetch independently complete official order/fill tapes, reconcile physical positions/cash and then apply native ledger events exactly once |
| `NoExecution` | Private complete body and an exact documented status/code combination | Retain the failed attempt and backoff; no synthetic fill or reservation release beyond the checked native state transition |
| `Unknown` | Response loss, malformed/incomplete body, partial/resting/mismatched ACK, unknown4xx,5xx or durability uncertainty | Preserve package key/payload and existing queue unknown binding; stop replacement submissions and reconcile |

NO ACK quantities can be signed, so the strict validator compares traded **magnitude** only after separately checking canonical side/action. A mismatched canonical action is not silently rewritten. `fillPrice` is the last side-relative fill price, not an average; reported totalCost and ALL metadata remain advisory until complete fill/cash reconciliation. Every successful ACK explicitly sets`requiresCompleteFillReconciliation:true`.

Known zero-execution mappings are deliberately narrow: exact AllOrNoneNotFilled,429RATE_LIMITED, documented401 authentication codes, documented403 permission/consent/account codes,404NOT_FOUND and422RELATIONSHIP_VIOLATION. Status alone does not establish rejection. Unrecognized400 and all409 claim/conflict states remain unknown, as do all5xx. A body carrying both an error and results is never accepted as zero execution. Existing persistent Retry-After is preserved; the method never automatically resends.

This layer does not mark an order filled from an ACK, classify collateral savings as income, infer existing inventory's basis, assign ownership of a historical fill, clear an unknown binding, or create a profit claim. Those are native production obligations.

## Validation and remaining integration

Ten focused offline tests exercise fixed wire mode/order/scope, gross and aggregate quantity caps, invalid/duplicate legs, signed-NO full-ACK requirements, successful one-write evidence, exact zero-fill rejection, unknown partial/malformed/unrecognized responses, replacement blocking, Authority denial before network, persistent429backoff and strict reason/index validation. They use loopback fixtures only. Diff checks pass locally; Linux compiler results are recorded in#127 before checked publication.

Before production use the execution owner must add separately reviewed native package reservations, source-bound settlement/conditional-EV proof, synchronized executable depth, exact package Authority, durable dispatch, explicit full fill-tape/account feedback, restart recovery, unknown-state reconciliation and portfolio risk handling. Existing ordinary production can continue while this candidate remains unreachable through its Authority.
