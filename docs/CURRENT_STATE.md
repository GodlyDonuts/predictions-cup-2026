# Moloch current production ownership — October 15

Moloch is the human-selected primary production bot. The human directed it to run across the tournament, manage inherited holdings when advantageous, and take over main/the primary directory. [Task #120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120) owns actual execution and fresh runtime evidence; [task #121](https://github.com/GodlyDonuts/predictions-cup-2026/issues/121) integrates source and retained Baal components.

The Rust production source is [moloch/live-runtime](../moloch/live-runtime/README.md); the ledger/replay engine is [moloch/native](../moloch/native/RUST_MIGRATION.md). The existing VPS service is `moloch-live.service`. A dated status file alone cannot establish a live PID or an accepted order.

During the all-market rollout, actual NO purchase39542185 received an HTTP200 response that failed the SDK's response-economics validator. The worker stopped without retrying it. At2026-10-15T08:20:40Z the execution owner reconciled that exact order as expired unfilled using a later complete owned-fill observation; all eight logical quotes were retired with no unresolved acceptance.

The execution owner subsequently verified the original683-event bank prefix against the new native identity and adopted28 inherited positions at exact open-lot principal, preserving the original manifest, history and reported-basis discrepancies. No historical fill was fabricated. Pure opening holdings remain outside the new-position forced-loss timer. The checked source `572034cb` passed81 native and33 live checks plus locked release builds.

The next actual order39613596 was a covered SELL of78 NO shares at0.285. Its HTTP200 response again tripped `ORDER_ACK_ECONOMICS`; no replacement was sent. The parent then fetched that exact known order and complete fills, bound it as open with zero fills, and cleared only its proven queue uncertainty. The SDK repair separates canonical order identity/size from advisory traded/remainder totals, while actual fill credit still requires complete official owned-fill receipts.

SDK source66e4ec97 passed65 checks and its release build. V11 was deployed at08:49:51UTC; an initial stream connection failure honored the persistent60-second token-mint backoff. Later PID106041 connected and obtained a successful cancellation acknowledgment for39613596, retained privately without authentication headers. The next pass stopped on `KNOWN_INTENT_RECONCILE_FIRST` before a repeated physical cancellation. The subsequent durable cancel-acknowledgment repair remembers confirmed cancellation across restart and waits for authoritative terminal GET/fills before releasing liability.

V12 started at08:55:14UTC as PID108423. At08:56:49.877UTC the execution owner verified three healthy cycles, nine official acknowledgments, three terminal orders and no execution STOP. The replayed bank had1666 events,32 logical orders and two actual fills. Realized lot-basis gain was2.340000 SUSQies, entirely `inheritedRealizedGross`; `newTradingRealized` remained0. At that observation attributed cash was15327.845, remaining inventory cost24674.495 and quote reservation2745.58. These are dated source-ledger observations, not a forecast of strategy profitability or an assertion that lot-derived profit equals official whole-account realized P&L. [Task #120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120) owns fresh process/account evidence; [review/publication #125](https://github.com/GodlyDonuts/predictions-cup-2026/issues/125) records the source review.

The [sanitized official-fill proof](../data/moloch-inherited-first-official-gain-20261015.json), exported at08:59:02.219590UTC, binds the two fills of covered sale39633229 to the private complete official receipt:78 NO shares at0.735,57.330000 proceeds,54.990000 inherited lot cost and2.340000 gross gain, with zero fees. The export excludes later fills and open marks. At09:00:13UTC the parent observed V12 still healthy after seven cycles, with16 official acknowledgments, five terminal orders and three total actual fills; that later count does not extend the two-fill profit proof.

Checked source3bc4eb82 adds explicit non-null ACK identity, platform-correct evidence durability, durable cancellation acknowledgment and an optional official relationship/collateral census through the same account transport. Optional census reads follow the first reconciliation/trading cycle; diagnostic output cannot stop ordinary trading. The shared SDK passed83 checks, live runtime38 checks and a locked release build, and the independent relationship payoff/scanner library31 checks. The native financial source is unchanged from the81-check adoption version. V12 continues running the earlier checked production source; deployment of the census candidate remains the parent's controlled follow-up. Relationship evidence and payoff certificates alone do not authorize package execution or establish profitable opportunities.

The funded allocation is 40,000 SUSQies, with 10,000 per market, maximum submitted quantity 10,000, and at most three fresh-market exposures in addition to attributed inherited holdings (28 at adoption, effective maximumMarkets31). The whole tournament is watched for discovery; inherited principal consumes the same total committed allocation. The original entry cutoff remains 2026-10-15T14:44:34.413Z. One shared queue permits at most 100 read admissions/minute with 600 ms completion pacing and 24 contest writes/minute with 2500 ms pacing; actual network latency lowers achieved REST throughput.

The human authorized inherited-inventory management, but historical cost basis and attributable profit require separate exact reconciliation. Old holdings, source journals, funded banks and unknown order state must remain intact. The prior Baal realized cohort is historical and must not be counted as Moloch profit. See [integration details and retained benefits](MOLOCH_MAIN_TAKEOVER_20261015.md), plus the [initial dated rollout](MOLOCH_LIVE_STARTUP_20261015.md). All older sections below are historical where they conflict with this current ownership/runtime direction.

# October7 06:30 UTC: foreign-held candidates no longer block discovery

Checked17b54400 host20 is live onsolecollector32364, freshbootstrap verified06:29:47UTC. Existing6verifiedBUYs/600NOshares/120.400principal, newR0 and allcoreV9journals/budget/activation remain unchanged. Explicitpre-intent BUY_TARGET_NOT_FLAT nowdefersonly thatpair andcontinues bounded discovery. Pending/unknown/durable/backoff/STOP conditions stillblock. Actualhostloop firstforeign/secondeligible,twoattempt/headroom andblockingstate tests pass, plus82runner checks. No foreignholdings adopted/offset, no added account client orrenewed08:46entry/Oct8 04:46exit cutoff. [Source-bound live evidence](../data/baal-expansion-live-20261007.json).

The goal remains active. New realized profit is0; old+.780closedcohort is separate. Next substantive work is economically supported quoting/inventory and the current mismatch between relative-value signal and unpaired holding horizon, not more model-count milestones.

# October7 06:17 UTC: exit-read prefilter live, six purchases verified

Collector25816 runs checkedcdb45eec host19/controllerV9. At06:16:58 UTC,6official BUYs are terminally reconciled:600ownedNOshares across6markets,120.400000principal, newrealizedP&L0.000000, no pending intent/unknown acceptance. Prior+.780closedcohort remains separately archived. Same10,000allocation,100shares/order,08:46entry cutoff/Oct8 04:46exit and one60READ/24WRITE queue.

The source-bound optimistic FIFO hint deferred28unprofitable exit-check packets in31evaluations;140is their potential minimum packet cost, NOT controlled actual-network savings. Fresh bulk prices cannot authorize orders; only a rejection upper bound is used. Missing/stale/foreign/corrupt hints fall through, and realized buffer/FIFO first-lot semantics are preserved. Existing account/depth/fullphysical byte checks and all journals/holdings remain.3hint checks plus82runner checks pass. [Actual source evidence](../data/baal-expansion-live-20261007.json).

Remaining throughput finding: BUY_TARGET_NOT_FLAT should defer a foreign-held pair without ending discovery; never adopt or offset foreign holdings. Higher turnover also requires a separately supported quoting/inventory policy. No scalableprofitability established and no goalcompletion claimed.

# October7 05:54 UTC: expanded trades reconciled; read priority repaired

Sole collector28048 runs522a9947 host18/controllerV9 under the same10,000 allocation and original08:46entry/Oct8 04:46exit deadlines. Both actual BUYs18776087(384/ex1073) and18785674(353/ex1042) are terminally verified:100NOshares each,9.000principal each,200shares/18.000totalcost. New realized P&L0.000; prior fullyclosed+.780 preserved separately. No pending intent at05:54:29. Known-order packets now wait for reserved read capacity before fresh receipts, preventing background capture from starving reconciliation; no rate-cap increase/client/order replacement. Physicalfullsource/actor/knownexpiredorder/risk fences apply to DELETE too.94producer/capacity/cancel/runner checks pass. [Source-bound evidence](../data/baal-expansion-live-20261007.json).

Current policy remains low-turnover:37source-qualified paired opportunities within237exchanges, one active intent, one lot per market and protected profitable exits. Raising capital alone does not createhigh-frequency alpha. Next work should reduce avoidable read/capture contention and implement/validate a separate economically supported quoting/inventory policy; do not force turnover or import old profit into new wallets.

# October7 05:34 UTC: 10,000-SUSQie Baal expansion live

Baal's sole collector24452 runs checked4435041a host17/controllerV9. The human-requested allocation increased1,000→10,000, order cap25→100, market100→1,000, race200→2,000 and active-market20→100;100shares/order and zero fees remain. First actual expanded BUY18776087 acquired100NOshares on384/ex1073 at0.090, exact principal9.000; complete official terminal proof verified05:31:02.397UTC. At05:34:02 the new run holds100shares, zero realized P&L and zero unresolved intents. Prior fullyclosed scaled cohort+.780 remains separately archived. No scalable-profit claim.

The earlier100-share pre-fetch rejection was affirmatively audited and reconciled with fresh official holdings/history; original actor/STOP and24closed inherited intents are preserved. V9 records proven clock-before-fetch blocks, releases only expired demonstrably unfetched reservations, and still halts on ambiguous actual acceptance. Preliminary historical checks use pinned metadata; initial and physical full-byte checks remain. Candidate rejection cannot starve discovery; at most2attempts/cycle within the same60READ/24WRITE queue,100msread/2500mswrite completion/backoff. Same08:46UTC entry cutoff andOct8 04:46UTC exit deadline, no renewed window. 3actual-source controller tests,16bridge,85runner and proof/pinning/sizing checks passed. [Live source-bound evidence](../data/baal-expansion-live-20261007.json).

# October7: human-requested10× capital expansion prepared

New separately bounded Baal run allocates10,000 SUSQies,100/order,1,000/market,2,000/race,100active markets and100shares/order, zero fees. It preserves23closed inherited intents and the original flat+.780 cohort as verified immutable history; the new run starts at zero realized P&L. Three sizing/partial-profit checks, two actual-source BUY/unknown/restart tests,16bridge checks and85runner checks pass. Activation is being installed; fresh live execution evidence follows separately. Single shared60read/24write limiter and source-age gates remain. No subagents/new keys/new API clients.

# October7 04:17 UTC: full official cohort flat with +0.780 realized profit

The complete attributed scaled cohort is now liquidated. Official Nevada SELL18591078 filled29NOshares for4.060 against4.060 cost; terminal fills/current signed position/scoped history and rounded cash consistency verified. Full economic journal replay reports zero remaining attributed shares, zero pending intents, zero fees and+0.780SUSQies net realized across the whole cohort. The two previous losing exits and all22inherited intents remain intact; none were excluded or reset. [Verified full-cohort result](../data/baal-full-cohort-profit-20261007.json).

Collector20832 remains running; new buying is disabled and the old entry cutoff remains expired. This is one measured profitable cohort, not proof of scalable edge, whole-account profitability or a winning competition bot. Next compare actual fill/depth/latency and capital turnover before a separately bounded prospective policy. Historical status below is superseded.

# October7 01:57 UTC: blocked intent reconciled; production remains stopped

GET-only collector15020 obtained fresh official account receipts01:55:50.978UTC. Full actor/timing replay and exact unchanged target holdings/scoped history validate closure of the expired blocked intent in a separate recovery record. Original actor/body and financialSTOP remain unchanged;68 shares retained, no recovery orders or incremental profit. Earlier host9 healthy first-cycle status below is historical: later final-gate failure stopped trading. The apparent API connection retries were a missing read-only receipt callback, fixed2233637f with86checks; no outage conclusion remains. Final-confirmation bridgeV5 candidate removes two redundant risk calls but is not deployed. Next import verified closure and validate reduced final-gate latency before any production restart. [Verified live reconciliation](../data/baal-blocked-intent-live-recovery-20261007.json).

# October7 01:27 UTC: repaired generic execution live

Collector9828 runs host9 from checked e4e7a764. Fresh cash/owner and first live cycle verified;68shares/+0.975 realized retained. No new order or profit. Eight-read BUY/five-read SELL preparation and final two confirmations retain20/5s freshness. Same serialized client uses explicitly coordinated100ms completion gap with persistent60READ/24WRITE caps,2500ms writes and existing backoff.29 regressions,85 runner checks, stale-read recovery and real-CPU host/restart passed. Prior V3 journal replay verifies0neworders and immutable source seals. Separate entry window01:22:57.957–02:22:57.957UTC, originalexit22:16:18.437 unchanged. [Actual live evidence](../data/baal-generic-v2-live-20261007.json).

# October6 20:45 UTC: three closed markets, 0.975 official realized gain

Full cryptographic and economic replay of 273 actor rows verifies 14 actual official orders: six BUYs and eight SELLs. The latest Ohio sale, order17352375, sold37NOshares at0.335 against0.325 original FIFO cost, adding0.370; the scaled run now has0.975 realized SUSQies, zero fees.68shares remain across three markets with15.780 cost. Open holdings and historical account losses are excluded from this realized figure; scalable profitability remains unproven. [Sanitized fill proofs](../data/baal-scaled-official-profit-20261006.json).

A reusable local evidence refresh now replays the complete original economic journal before publishing exact attributed gains; it performs no API calls and exports no raw account receipts or order keys. The original18:01 BUY cutoff remains unchanged. The full-depth exit and paired-custody upgrade is being checked separately while collector7780 continues managing existing positions.
# October6 19:56 UTC: official realized gain0.605; profitable exit recovery verified

Baal has13attributed official orders:6purchases and7SELLs. Exact terminal fills/FIFO proofs verify0.605SUSQies realized, fees0;105shares remain in four markets at27.805nominal cost. This is closed-portion profit, not a claim about liquidation of the remaining portfolio or scalable profitability. [Official proofs](../data/baal-scaled-official-profit-20261006.json).

Collector7780 runs hostV5 without errors after a resource-limit failure. A complete67.31MB financial journal exceeded its64MiB reader while the original controller allowed128MiB. The new reader matches128MiB and preserves all bytes/crypto/accounting. Funding synchronization repairs terminal proof committed before the old reservation install failed. KnownSELL17172790 was not replaced;20combined recovery checks, an independent production-failure check and92runner checks pass. Original unowned BUY window18:01:56.310 remains closed; owned exits continue. [Recovery](../data/baal-scaled-journal-recovery-20261006.json).

The price-bounded liquidity planner found29paired displayed shares versus2 under the1%rule in one capturedMichigan packet; no fills or profits are inferred from it. A fully funded counterpart pilot passes33coupled checks but remains dormant: it needs current positive ownership and a custody-preserving exit path, including any newly acquired counterpart. OldRIandDEpositions are now flat and cannot be revived from historical seeds.

# October6 18:13 UTC: six official positions preserved; faster risk verification live

Baal owns162NOshares across six markets with verified cost32.930 and zero scaled realized profit. Collector28156 is running hostV3 without errors. Exact-full-bytes projection reuse measured about218ms→39ms on the actual22.62MB sealed actor source; runtime reports80cache hits/7misses, with complete bytes/SHA, owner/STOP and fresh cash/risk checks retained. Original controller/budget prefixes verified unchanged. [Live evidence](../data/baal-scaled-projection-live-20261006.json), [six terminal fills](../data/baal-scaled-official-six-fills-20261006.json).

The original unowned BUY window ended18:01:56.310UTC and was not renewed. Existing positions remain managed for protected exits. No paired counterpart purchases, new entry activation, scalable profit or settlement gain are claimed. An independently checked exact-bid exit proposal found zero additional one-tick opportunities in the sealed captured window; five positions lacked post-proof public depth coverage.

# October6 17:23 UTC: scaled production owns57shares across two markets

The active1,000SUSQ allocation has two verified official NO purchases:29 Rhode Island Democratic Governor shares at0.085 (cost2.465), and28 Delaware Democratic Senate shares at0.095 (cost2.660). Total attributed purchase cost is5.125; both positions remain open and the scaled run has zero realized profit. Remaining package-risk reservations are51.305, for total full-nominal exposure56.430. These reservations are funding headroom, not purchased hedges or guaranteed profit. [Verified inventory and fills](../data/baal-scaled-official-inventory-20261006.json).

Actual collector25896 read traces identified a limiting defect: two unsuccessful protected-exit preparations consumed10GETs/about14seconds before entry ranking, aging previous bulk hints beyond the20second limit. A separately versioned scheduling improvement is being checked; fresh negative hints may skip only preparation reads, while trade dispatch and order reconciliation retain actual source verification. Original run/accounting and18:01:56.310UTC BUY cutoff remain unchanged.

# October6 17:09 UTC:1,000SUSQ scaled production verified;29-share fill

The newproduction run bought29NOshares on384/ex1073 in officialorder16817788/fill70692281 at0.085, actualnominalcost2.465, filled17:08:28.005UTC. Full terminal fills/signedposition/history andbudget proofverified; collector25896 is actuallyrunning withno recordederrors. Sourceproofddb49405... records the29-share inventory andfull28.710 packagebudget (2.465owned+26.245unusedhedge-riskreserve). No automaticcounterparthedge orscaledrealizedprofit is claimed. [Official fill](../data/baal-scaled-first-official-fill-20261006.json).

Human1,000SUSQ allocation is now operational under25/order/100market/200event/max100shares/max20markets. BUYentry closes18:01:56.310UTC; owned cleanup/exits remain scoped. Existing4closedpilottrades+.015 andallsource/bank historiesremainpreserved. Oneactualaccountqueue, no newcredentials/services.

# October6 17:04 UTC: separately bounded1,000SUSQ scaled run started

Collector25896 started17:04:12UTC for newrun baal-scaled-no-20261006-human, with1,000nominalcapital/25order/100market/200event/max100shares/max20markets. NewBUYentry ends18:01:56.310UTC; owned reconciliation/exits remain scoped after entry closes. It is restoring before fresh account and execution verification. No new scaled order is claimed yet. [Configuration](../data/baal-scaled-live-configuration-20261006.json).

Exact larger-share FIFO/partial/resized order accounting, sharedfunding and streamedactualclient components passed77integrated checks plus49independentcontroller/verifier checks;85runner checks pass. The original4closedcurrentpilottrades net+0.015 and allpriorbank/source records remain unchanged. Legacy retirement pins verifiedcurrentowned/reserve0 before the new actor; oldmanualholdings cannot be adopted. No newclients/keys/services, zero fees, same60read/24write completion-paced accountqueue.

# October6 15:53 UTC: official NO round trip verified profitable and retired

Collector27692 completed terminal reconciliation of SELL16539746/fill69538369: one NO share sold0.080 at15:22:50.482UTC against BUY16297733 cost0.070. Exact own fills/currentflat/scopedhistory/roundedcash-consistency verified net+0.010, fees0. The actual repair overlay releases only currentinventory/unused0.920 and preserves original0.070 historicalcost, cumulativepurchases, orderkey, body, and all source/journalprefixes. No replacementSELL orhedge was sent. Original3mediantrades net+0.005 plus this roundtrip gives+0.015 across4closedcurrentpilottrades; this tiny sample does not prove scalable edge or overallaccountprofit. [Official roundtrip](../data/owned-no-official-roundtrip-20261006.json).

The collector is live in GET-only cleanup/collection mode after retirement. Human-authorized next-policy capital allocation is1,000SUSQ with25/order and100/market ceilings, not yet a larger active writer. Quantity1..100 accounting/partialfills is in implementation before using that allocation in production.

# October6 15:48 UTC: reported profitable sale under terminal repair; capital allocation increased

The protected writer submitted one NO SELL16539746 at15:22:50UTC. Its original raw acknowledgment reports1share filled at0.080 versus0.070 basis (apparent0.010 gain), closed/rem0, with non-null neutral SELL economics. The old null-only rule halted semantic reconciliation; no replacement or hedge occurred. New explicit SELL helper/controller/retirement revisions preserve original ACK/prefix/key/body/window and verify only the observed zero-collateral release pattern.40 component and8 hostchecks pass;85 collectorchecks pass. GET-only collector27692 launched15:48:23UTC to verify terminal order/fills/currentflat/history and retire the unused0.920. Actual additional realized gain is not claimed until live proof. [Recovery configuration](../data/neutral-sell-recovery-configuration-20261006.json).

The human directly requested more capital. A separate1,000SUSQie next-policy allocation is saved with25/order,100/market,200/event and100shares/order ceilings, zero fees. The old one-share history and2SUSQie pilot are not reset; current cleanup cannot issue larger trades. Pure sizing7checks pass; multi-share receipt/accounting support is being implemented before the increased allocation can be used in a new execution policy. [Allocation](../data/baal-capital-allocation-20261006.json).

# October6 14:54 UTC: protected owned-NO exit trial installed

Collector24792 started14:54:37UTC with the original verified one-share Rhode Island Governor NO lot and a separately bounded one-SELL request ending15:54:12.821UTC. It is restoring source history before fresh execution verification. The old paired review/entry clocks are unchanged; this exit request does not buy or replace shares. It requires fresh complete account/depth evidence and at least0.005 protected net gain after actual0.070 original cost. Fresh negative bulk hints only avoid unnecessary reads; they cannot authorize a sale. Twelve host checks,32 supporting checks and four independent review checks pass. [Configuration](../data/owned-no-profit-configuration-20261006.json).

An explicit counterfactual comparison pause preserves both Kansas paper holdings and all banks/journals. Exact200.75MB ledger/93.61MB consumed public-prefix accounting verified in3.68seconds; the prior economic cold replay took399.9seconds. Paused valuations are dated frozen snapshots, not current marks or official holdings. Official account collection and original broker receipts remain separate. Seven adapterchecks and85 runner/integrationchecks pass. [Preservation evidence](../data/frozen-comparison-pause-20261006.json).

No new actual SALE or realized gain is claimed. The original three closed official pilot trades remain+0.005; the new NO holding has zero realized profit.

# October6 14:17 UTC: first generic NO fill verified in the official account

The corrected collector15272 completed full scoped reconciliation of original order16297733 and fill68545127: one Democratic Rhode Island Governor NO share (market384/ex1073), actual0.070 cost, filled13:43:44.169UTC. The original raw acknowledgment, terminal order, complete signed REST fills, scoped history and current NO position agree. Original prefix, key and clocks remain intact; no replacement or hedge order was sent. The actor retains its0.920 remaining reservation, and global cash/activity is still not claimed exactly verified. [Official fill evidence](../data/paired-first-no-official-fill-20261006.json).

Collector15272 had16 healthy cycles/noerrors at14:23:38UTC. Its previous delay was primarily6m40s cold settlement-paper restoration; steady cycles are14–19seconds. New owned-exit controller/evidence bridge/accounting checks pass46 parenttests, and independent source review passes16 controllerchecks. They are not activated yet. The protected exit policy needs at least0.005 verified net gain; there is no forced trade. Original three closed median trades remain+0.005 actual, and the new NO lot has zero realized profit.

# October6 14:07 UTC: corrected recovery host deployed; terminal verification pending

Collector15272 started14:07:08UTC and is restoring preserved history. The sole account queue remains unchanged. A separate GET-only recovery host validates the original neutral NO acknowledgment and appends verified reconciliation to the same actor/funding journals. It cannot submit, cancel, hedge or replace an order. Its8 integration checks include real wall-clock separation between candidate qualification and actor initialization: both original plan hashes remain distinct and preserved. Original review13:48:14.155 and outer14:26:11.789 expiries were not extended. [Configuration](../data/neutral-ack-recovery-configuration-20261006.json).

The previous startup ownership error occurred22ms after the engineering STOP request interrupted restore; no evidence of portfolio corruption or stale financial ownership was found. The new collector has no recorded startup errors yet, but fresh terminal order/fill/position verification is still pending. The original178451-byte paired actor prefix and0.990 reserve were preserved.

Runner admissions now reserve the auxiliary window before fresh account reads, preventing a newly nominated pair from spending a second auxiliary allowance in the same cycle. A cold idle capture can nominate its first candidate.82 runner and7 selector checks pass. New owned-NO exit signal and scoped SELL reconciliation have21 checks; they do not dispatch orders. No extra clients, credentials or services were created, and no paper marks are presented as realized gains.

# October6 13:51 UTC: first generic NO order acknowledged; neutral-economics repair checked

The new generic selector obtained fresh rules/depth for384/385 at13:43:35UTC, protected pair cost0.925 with31/187 eligible shares. Actual order16297733 on384/ex1073 returned a closed one-share NO BUY acknowledgment at0.070. The original raw acknowledgment reports a neutral non-null ALL economics object: no collateral, relationships, advances or redemption. The old verifier required null, so its semantic latch correctly retained the whole0.990 reserve, but unnecessarily prevented scoped terminal reconciliation and the hedge. No replacement/second-leg order was sent. The source review expired13:48:14.155UTC without an extension.

New isolated neutral-economics/verifier/controller revisions validate exact original number lexemes and terminal fills/positions/history. Migration preserves the original journal prefix byte-for-byte and pins its descriptor on every restart.141 independent correctness checks pass; a private offline audit also replayed the actual original journal with zero network calls. These revisions are checked but NOT deployed yet: terminal REST reconciliation, lot ownership and paired profit remain unverified. Collector28804 has fresh running cycles and no runtime errors. [Actual acknowledgment finding](../data/paired-neutral-ack-finding-20261006.json).

# October6 13:39 UTC: generic pair selector installed; live restoration pending

A separately bounded version2 host can select one fresh source-verified opposing-party NO pair from all237 approved markets. The read selector reuses current bulk publications and the existing auxiliary capture slots; no extra account clients or services were created. Selection is immutable within a run, both legs receive full funding, and prior expired zero-order trials and every journal remain preserved. New run sig-pilot-paired-generic-20261006-human ends14:26:11.789UTC; its read-source plan ends13:56:11.789UTC. These clocks do not extend the original median or fixed-Florida activations.

Collector28804 was actually verified running at13:39UTC with empty startup logs while restoring original history. Fresh connection, candidate nomination and paired fills are not yet verified. Paper28488 and dashboard29228 were verified present. The original official pilot has three closed trades netting+0.005SUSQies, zero fees: this tiny sample does not establish profitable scale. [Configuration](../data/generic-paired-live-configuration-20261006.json).

A separate exact-decimal NO liquidation signal checks original buy proofs, current executable complementary ask depth, two-leg protection and minimum0.005 net margin. It has14 correctness checks; it does not dispatch sells or claim profit from test data.

# October6 12:42 UTC: official book shape corrected; zero-spend trial preserved

The live bridge wrongly required top-level marketId, which the official CombinedMarketOrderbook response omits. The genuine documented/observed shape contains exchanges, overround, hasArbitrageOpportunity and tournament contexts. Five primary-schema regressions now pass; exact endpoint, tournament, expected exchange, depth and contradictory-ID checks remain enforced. This was an integration defect, not a missing price signal.

The first trial initialized at12:29 after a fresh0.990 protected pair screen with5/1232 eligible shares, but the schema guard blocked preparation. It recorded **zero orders, zero fills and zero cost**. All journals remain in the original run; none were reset or called unfilled orders. The corrected run is sig-pilot-paired-book-schema-20261006-human under the same authority,2capital/1share constraints and **unchanged13:11:07.326UTC outer expiry**. Collector14316 has started its existing cold restore. No paired dispatch is claimed yet. [Repair proof](../data/paired-book-schema-repair-20261006.json).

# October6 12:11 UTC: small official gain; bounded paired trial installed

The third official close sold Rhode Island388 one share at0.160 (order15942338/fill67047007,11:25:01.282UTC) against0.155 cost. Three round trips now net **+0.005 SUSQies realized**, zero fees, with no original pilot holdings or uncertain intents at controlled shutdown. This is a tiny sample, not a proven profitable strategy. [Official evidence](../data/median-official-third-roundtrip-20261006.json).

Collector18440 was launched with a separately bounded one-pair REST writer inside the same process/queue. It is restoring existing records before fresh live reads. New original-median BUY entries drain through an explicit operator marker; its original banks, accounting, owner and13:01:24.019UTC expiry remain unchanged. The new request permits one Florida Senate pair354/355, one share per leg,1/order and2allocated capital, ending13:11:07.326UTC. Actual initialization further binds the short fresh-rule review deadline; no paired order is claimed before fresh source and actual fill verification. [Configuration](../data/paired-live-configuration-20261006.json).

The generic cached screen found37 opposing-party matches from87 roots; five price leads require fresh rules/account/books. The strongest Florida Senate cached protective cost was0.975, while a subsequent actual10:53 depth capture screened0.980 with9/18 eligible shares under the1% depth fraction. The REST plan preserves source costs and reserves at most0.990, retaining at least0.010 ordinary conditional payout margin while improving hedge completion headroom. No unconditional arbitrage or realized paired profit is claimed.

The shared fund counts prior YES and new NO exposure together, reserves the complete pair before its first read/dispatch, blocks competing writes during unknown/hedge phases and retains orphan liabilities. Raw streamed receipts preserve original clocks/number lexemes in bounded private memory. Reconciliation uses complete scoped order history and terminal fills; rounded cash remains an explicit interval. Source, queue, restart, cancellation, STOP and response-based persistent backoff checks passed; existing81collector lifecycle checks also passed. See [paired execution work](PAIRED_EXECUTION_PROGRESS.md).

# October6 10:32 UTC: exit repair verified; pilot realized net zero

Collector20172 completed cold restore and fresh bootstrap and submitted the latched Michigan exit. Official order15801594/fill66455402 sold one share at0.150 on10:30:12.989UTC against0.160 cost:−0.010. This offsets Rhode Island's +0.010. **Two closed round trips have zero net realized profit; no profitable strategy is established.** The controller subsequently bought one Rhode Island share at0.155 (order15805641/fill66471755,10:32:01.510UTC) under the original bounded activation. No liability was discarded. [Official evidence](../data/median-official-second-roundtrip-20261006.json).

The unchanged08:59:42.428UTC prospective cutoff was replayed through a later sealed capture in34.14s,39,792 events. All three cases have zero closed trades. Native/capped controls each hold a0.160 Rhode Island share; the strict uptrend case rejected all15 funded candidates because their recent segment declined and holds none. Avoidance is not profit. [Subsequent comparison](../data/median-uptrend-cap-subsequent-20261006T102335862Z.summary.json).

# October6 10:18 UTC: proposal clock defect repaired

The original V2 proposal constructor sampled time twice. A one-millisecond advance produced a 120001 ms lifetime and failed its exact 120000 ms proof check before committing a SELL. Offline reproduction and five focused regressions establish this defect; the live halt followed Michigan's holding-timeout latch, but the old generic failure log did not retain the original exception. A separate synchronous clock adapter captures one timestamp and restores real time immediately after proposal creation. It leaves the original controller, strategy bank, ownership, accounting and activation expiry unchanged. Thirty-eight controller, clock and runner checks passed; independent review found no blocker.

Collector9412 was stopped through an attributed engineering marker with no open or uncertain intents. Collector20172 was launched with the checked adapter pinned in its193-file graph; cold restore and fresh account bootstrap are still pending. Michigan remains one owned share at0.160 with its timeout obligation retained. Do not claim the repair has executed an exit before observing an official fill. See [deployment evidence](../data/median-proposal-clock-deployment-20261006.json).

Opposing Alaska Senate NO contracts have a conditional ordinary settlement floor of1 per pair. A fresh paired depth capture at09:58:30UTC screened a0.985 cushioned pair cost, or0.015 conditional surplus, with43/26 eligible shares at the1% depth fraction. This was read-only evidence and has expired; no paired orders or profit are claimed. Refund, administrative resolution and failed-hedge exceptions remain explicit.

# October6 09:20 UTC: first profitable official roundtrip; V2 live

Collector9412 is freshly running with same-run V2 policy bank `da6162418dc06cb28e1873b8f1b0613f8b43f7b1c49b3e61e32cc8bd4ead8854`. Original activation/budgets/13:01:24.019UTC entry expiry are unchanged. New buys retain at least one tick below the original reference; protected-market candidate selection now includes all planned IDs. Prior owned costs and journals survived migration. See [V2 behavior and checks](MEDIAN_CAP_V2.md).

Official Rhode Island388 roundtrip: BUY0.170 at07:37:48.170UTC, SELL0.180 at09:16:43.230UTC (order15622673/fill65703409), **+0.010 realized SUSQie, zero fees**. Three orders/fills reconciled, zero unconfirmed intents. Michigan306 remains one share at0.160 cost. At09:20:18UTC its fresh bid value0.155 produced **+0.005 observed total pilot P&L**; this mark is not a sale. Original Rhode Island entry was uncapped; do not attribute the win to the new cap. One roundtrip is not repeatability or competition-wide profitability evidence. [Official record](../data/median-first-official-roundtrip-20261006.json).

Broad causal bulk diagnostics found1,477 candidate price paths:516 target touches,208 stop touches,103 horizons,646 data gaps,4 unfinished. Even optimistic quote differences were negative overall; these are not fills or funded returns. A separate fixed strict-uptrend-plus-cap comparison is staged with future cutoff08:59:42.428UTC. It remains paper-only until measured results warrant a decision.

# Historical: two official purchases; no completed exit

The existing pilot has two reconciled one-share BUY fills: Rhode Island Republican Senate388/ex1077 at0.170, and Michigan Republican Governor306/ex995 at0.160 (order15443862, fill64960172, 08:04:28.289UTC). Total attributed cost0.330; zero unconfirmed intents and zero realized official profit. No exit or scale-up is claimed. Collector20964 remains running under the unchanged bank and13:01:24.019UTC entry expiry. See [two-buy evidence](../data/median-official-two-buys-20261006.json).

The fixed entry-cap paper comparison extends its original07:37:20.792UTC cutoff. Through the08:05 capture, both wallets have two open positions/zero closes: observed total P&L -0.015 baseline versus -0.010 capped. The cap delayed Michigan306's acquisition from0.165 to0.160; neither wallet is profitable. The actual production purchase at0.160 is separate evidence and did not use the new cap. See [subsequent evidence](../data/median-entry-cap-subsequent-20261006-0805.json).

Primary [Rhode Island research](../research/ri-senate-primary-20261006/README.md) confirms the federal general-election contract and a recent UNH poll with a substantial Reed lead. No calibrated Republican win probability supports the0.170 purchase as settlement value; its rationale remains two-hour price reversion. The live policy, inventory and research clocks were not rewritten.

The [offline Rust restore utility](RESTORE_REPLAY_RESULTS.md) verifies every original KS/TX checkpoint and archived wallet exactly. Kansas took37.80s versus the captured host's294.42s at a slightly different frontier; Texas speed improvement is modest. It is checked but not deployed over live holdings.

# Historical: first official median purchase reconciled

Discovery v3 and shared protected-receipt servicing are checked in main `5e57aa3ff5a7808ab6e6946c09793c9e7bc8a02e`; 107 integration checks pass. Collector20964 restored, refreshed its execution bootstrap at07:35:47UTC, and runs alongside paper28488/dashboard29228. Pilot `sig-pilot-median-20261006-human` has one confirmed BUY: market388/ex1077, order15377038, fill64685852 at07:37:48.170UTC, one share at0.170, attributed cost0.170. Original reference0.175. Zero unconfirmed intents; zero realized official profit. The 07:49:21 fresh bid snapshot valued inventory at0.165 (-0.005 observed total P&L), not an actual sale. Reverify current reports. The existing bank and **13:01:24.019UTC entry expiry** remain unchanged; no activation renewal or scaling occurred. See [official evidence](../data/median-first-official-fill-20261006.json).

The supplied entry-price-cap review was implemented as an isolated offline comparison with a preregistered future cutoff. It tightened7/14 historical limits but left fills/results unchanged: both arms -0.065 closed-trade P&L, three unpriced open positions. 25 checks and identical-input hardened-source reruns pass. Initial future wallets have one open position/zero closes. No profitability improvement or live strategy promotion is claimed. See [entry-cap evidence](MEDIAN_ENTRY_CAP_RESULTS.md); earlier trend filters were also not promoted.

# Historical: small official median pilot armed

Under the latest explicit human instruction to run the small trial now, `sig-pilot-median-20261006-human` is armed in the sole collector. Fresh complete account preflight and bootstrap passed; bank `af566e22332573e759e6dbd45fd4c6dcf3ab4d111362a245e51bc19c66e2fd70` binds the original one-tick paper policy. New entries expire **2026-10-06T13:01:24.019Z**; this is a separate bounded activation, not renewal of the expired settlement pilot. One-share orders, 1 SUSQie/order, 10 aggregate cost, 25 cumulative buy cost, three markets and zero fees remain enforced. Current official status is waiting for a qualifying signal, with zero orders and zero attributed positions/profit. See issue comment [6011196287](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-6011196287) and [trial behavior](MEDIAN_PRODUCTION_TRIAL.md).

Checked main `bd93bb2e3298a10cf72afa6ad65bf1dad837a5e9` includes saved-bank read-back verification. Execution/reference/preflight/activation checks pass 42/42; collector lifecycle/discovery checks pass 101/101. Observed persistent processes: collector25516, paper28488, dashboard29228; reverify current commands and reports. A bounded file-only median observer also preserves the original paper cohort. Its two +0.005 closes were offset by a -0.010 timed exit, leaving realized P&L zero and one open position. No sustained profitability is established.

Next experiment preregisters two causal trend filters at **2026-10-06T07:08:27.849Z** in `data/median-trend-experiment-plan-20261006.json`. Historical comparisons are development diagnostics; independent future wallets must evaluate subsequent captures. The running official policy and paper accounting remain intact.

# Historical: market-wide median paper operation

Checked mainb889c641889c43af2bcd161983ed4b232b7683bc adds candidate reads through the existing single collector. Four protected depth20 slots remain first; at most two auxiliary candidate books run every third cycle under the existing account queue, freshness and headroom. Real receipts for274/361 passed causal publication. Collector27944 was freshly running with zero errors; paper8732/dashboard29228 remain. Reverify processes and timestamps.

Frozen median cohort baal-median-prospective-20261006-v1/bankfe61bfcee3e0fbc2bda5170de3fff0f3841c82d451529df2a20403a027098331 retains independent1/2/4tick wallets and zero fees. The one-tick wallet has three actual prospective counterfactual purchases:274cost0.105,305cost0.860,361cost0.165; cash98.870, zero realized and last complete liquidation−0.015. Other cases remain100cash/zero fills. These are paper positions, not contest orders or profitability. Fresh captured contract metadata identifies Montana Democratic Senate, Michigan Democratic Governor and Minnesota Republican Senate respectively.

The original600s observer repeatedly spent minutes on cold verification. A separately pinned same-bank code continuationf3d5d78... now runs declared3600s sessions and preserves the original bank/config/binary/financial journal. Independent review and actual205-checkpoint parity passed:92.69GB repeated prefix reads became307MB with identical funded states; measured125s became45–47s. Runtime revision/pins live under LOCALAPPDATA/SigTradingCup/paper-runtime/median-observer-revisions. Exact lease/STOP, input/source checks and revision provenance precede live consumption. See data/median-code-continuation-live-20261006.json; keep this approved paper operation healthy without duplicate owners or accounting reset.

Parent tested a separate entry-reference exit hypothesis across the original sealed86.277hours/31,447books in4.231seconds. All six funded controls are retained. It worsened shallow realized results to−0.015 and left unpriced inventory; the deeper result stayed+0.010 with fewer trades. Reject that change; do not deploy or tune it into a historical-profit claim. See MEDIAN_REFERENCE_TARGET.md. Profit-only work remains active: actual future exit outcomes, economic edge and necessary coverage/performance repairs. Unrelated migration/visual work is frozen. Original contest activation remains expired; no new official orders were sent.

# October6: native receipt paper purchase and dashboard

The separately frozen Rust receipt observer bought one subsequent Kansas YES share at0.350 with zero fees; each correlated case retains99.650 cash,0 realized profit and−0.005 observed estimated liquidation. Old wallets remain unchanged. The deployed Next dashboard shows its actual holding/fill and suppresses stale quote marks. This observer uses existing captured sources only, with no API client or persistent service; the full Rust account-service migration remains incomplete and contest activation expired. Nine original/native owners are now in the migration inventory. See [the native observer and exact live evidence](NATIVE_RECEIPT_OBSERVER.md).

# October5: accelerated paired-contract paper and Rust transport

Baal now runs two separately frozen Michigan Governor dual-NO paper policies inside the single collector. Full two-leg funding, later receipts, observed depth, zero fees and failed-hedge accounting are enforced. The original cohort holds a0.975 package with zero realized profit; its0.025 ordinary settlement surplus is conditional and not cash profit. Virtual-clock replay processes81.5 captured hours in about9.7seconds while preserving coverage gaps and exact original journal parity.125 correctness checks pass. Contrary Texas polls now block new fundamental entries, and two expired flat hosts are retired with accounting preserved. No contest activation was renewed. See [the measured sprint results](PROFIT_SPRINT_RESULTS.md) for results and limitations.

# October5: funded fundamental-value paper wallet

A separate future-cutoff wallet now runs the existing Rust settlement-value policy inside the single collector. It uses Governor-specific historical error with the original primary-source clocks, later-request one-share fills, zero fees and exact funded accounting. It keeps positions according to current fundamental value rather than a two-day timeout or price-only stop. The original research/bulk banks and journal prefixes are preserved; the bulk entry drain remains in force.115 correctness checks pass. The original contest activation remains expired, and profitability is not established. See [implementation and economic limits](FUNDED_SETTLEMENT_PAPER.md) and the subsequent paper evidence in data/funded-settlement-paper-live-20261005.json.

# October5: independently sourced Texas governor paper experiment

New unowned bulk quotes are now draining with durable operator boundaries; prior liabilities and inventory remain. Texas produced its first subsequent one-share paper fill at0.295, with zero closed trades and−0.015 estimated liquidation P&L at19:24UTC. Governor-specific history now measures231 races/800 observations and178 earlier-year-only diagnostic forecasts; it is a later candidate, not an overwrite of the active bank or proof of market profit. A new frozen one-share Texas Democratic governor cohort uses three primary polls, including the contrary Emerson result, an explicitly unvalidated and widened governor error proxy, and only subsequent executable receipts. It shares the existing paper worker and four book slots; prior Kansas/West Virginia holdings, banks and journal prefixes remain separate. No profitable result or official activation is claimed. See [source inputs, sensitivity and execution limits](../research/current-polls-v3/README.md).

# October5: fill-conditional models and funded quote replay

Quote-placement features, separate arrival/filled-value models, and an exact shared-capital replay now expose adverse-selection losses and unresolved quote liabilities. Both join-price and inside-price selectors choose zero later trades; exact-exit sensitivity reduces losses but leaves them negative. The funded inside-price development control realized−0.040 while still holding0.925 cost and2.060 unresolved reservations. No profitable strategy or official activation is claimed. See [measured results](QUOTE_OUTCOME_RESULTS.md).

# October5: fresh execution receipts restored; longer horizons show no qualified edge

A depth200 substitution in the last collector change invalidated the frozen depth20 publisher. That regression is corrected and checked against the actual native publisher. At17:54UTC collector8616, paper3976 and dashboard20092 retained the original banks/accounting and one account queue; the overdue spread position closed at−0.020paper SUSQies. Kansas showed+0.005estimated liquidation P&L and the older research wallet+0.025including its prior+0.020realized gain; correlated latency copies must not be summed. Neither the bulk wallet nor the supported5/30-minute forecasts established new profitable execution. The fixed two-hour purged comparison has insufficient chronology. No contest activation was extended or order submitted. See [the measured results and regression recovery](LONGER_HORIZON_RESULTS.md).

# October5: market-wide bulk history and inside spread quotes

October5: a separately frozen coupled paper cohort now uses causal bulk-price history across all237 exchanges and one-tick-inside paired quotes. Fresh full-book depth and account eligibility still control quotes and exits. Existing read slots and wallet histories are preserved. The original-source replay processed21,813 events in39.52s but had no fills and missing quote-window tape coverage; no profit is claimed. The 87 integration checks pass. The original official activation remains expired. See [implementation and measured limits](COUPLED_BULK.md).

# October5: shared-wallet implementation and collection repair

October5: repaired tape selection and started a shared-wallet spread cohort. The full237-exchange tape scope was already correct; the runner incorrectly selected only its old four markets. Exact recovery receipts for364/376 resolved all four blocked quotes without fills. A new separately frozen coupled YES/NO paper wallet models the documented immediate one-SUSQie pair payout, retains unmatched inventory risk, and uses zero fees. Historical original-source replay processed7,567 events in23.64s: one hypothetical purchase, zero completed pairs and zero realized profit. No production activation or profit is claimed. Windows sharing-denial retries and preserved polling-code archives address actual collector/restart failures. See [current implementation and measured results](COUPLED_SPREAD.md).

# October 5: broader passive cohort and actual loss attribution

A new independently frozen16-market passive paper cohort now shares the same collector, after original-source screening of237 exchanges. The narrow cohort's pending quotes were fully reconciled before retirement, with its bank/journal preserved. Actual prior bot loss is−0.380: two losing markets cost0.500 and winning markets earned0.120. No new official orders or demonstrated profitability are claimed. [Execution economics and implementation](EXECUTION_ECONOMICS.md). Older cohorts and timestamps below remain historical.

# October 5: stable spread paper implementation

The losing price-entry engines are retired. A new frozen passive paper cohort joins stable prices, seeks a one-tick covered exit and reconciles strict anonymous-trade projection windows. It runs inside the existing collector, with zero fees and no extra services. Spare reads now build continuous history; existing official and source-probability risk retains priority. **No profitability is established and no new official orders were sent.** See [implementation and measured evidence](STABLE_SPREAD.md). Older timestamps and results below remain historical.

# October5: native receipt exits implemented; current economics still negative

The separately frozen `probability-receipt-native-v1` policy matches the empirical polling paper strategy:1 share, zero fees,1% depth, adverse tick, profit0.020, stop0.050, two-day horizon and15-minute cooldown. Loss exits remain latched after insufficient depth, research expiry, restart and price recovery. Actual fill time supplies holding age. The new controller retains the same attribution, fresh reconciliation and uncertain-acceptance fences. Ninety targeted correctness cases passed; the original native binary/banks/accounting remain intact. [Implementation](../execution/receipt-production.md).

At12:48:34UTC the new native binary processed actual later public depth and the existing Kansas paper holding: HOLD, cost0.345, full liquidation0.330, net−0.015. No API call, contest order, realized profit or activation occurred from that diagnostic. The candidate is ready to use the measured exits; short-term profitability still lacks evidence.

Collector6904 was absent by12:31UTC; its last saved status was a graceful stop at12:28:31UTC with no error or retained STOP cause. Root cause is unresolved. It was recovered as16352, then the checked lifecycle fix was deployed with its own explicit STOP and preserved activation. Current PIDs are recorded in data/current-runtime-pids.json and must be rechecked. The new independent lifecycle record distinguishes signals/markers from status-publication failure; the latter now closes the source as a failure. Paper8236/dashboard20092 stayed alive, and no extra service or account client was started.

# October5: one empirical polling strategy now trading in paper

Following the human's instruction to focus exclusively on profitability, one separately frozen Kansas paper cohort is active: `calibrated-kansas-poll-20261005-v1`, market356 YES/exchange1045. It uses independently matched historical polling/final outcomes, individual-poll dispersion with equal total weight per race, and the original September15–16 Emerson poll and verified ballot. Model construction and chronology are documented in [poll calibration](../research/poll-calibration-v1/README.md). Estimated settlement value is not realized trading profit.

At12:16UTC both latency wallets had one local counterfactual fill at0.345, no closed trades, zero realized P&L, and−0.015 depth-aware liquidation P&L after adverse ticks and spread. All fees are zero. These wallets share one opportunity and are not independent samples. The earlier frozen wallet remains separately accounted, with its1-share West Virginia position and historical+0.020 realized result intact. No contest order or entry-window extension occurred. The existing single paper worker hosts both cohorts; read hints reuse the same collector slots and retain held inventory first. Exactly three persistent services remain.

Current work is strategy P&L, subsequent executable receipts and necessary fill/exit correctness. Broad engineering, repeated parameter banks and cosmetic dashboard work are not the current task. The original official pilot remains expired. Deployment must follow evidence from the same trading policy rather than substituting retrospective directional accuracy or an estimated settlement edge for actual net performance.

# October5: entry window closed; recovery and new polling assessments

At 11:19 UTC the three previously recorded service PIDs were absent. The last saved collector report was October4 23:43:43 UTC; its `running` field was not current process evidence. The cause of the disappearance remains unresolved. The original pilot entry window expired October5 03:59:22.131 UTC. Services were recovered without extending that window or changing the frozen production bank, attributed inventory or journals.

The controller now derives explicit `entry_window_closed` status from the original activation timestamps, while preserving blocked/uncertain states and owned-inventory management. This repairs a misleading status display; it does not grant dispatch authority. Targeted controller, projection and assessment checks passed 23/23. The recovered collector, file-only paper worker and local dashboard were observed alive; production reported zero submitted orders, zero unconfirmed intents and zero realized P&L for this pilot. Previous official net−0.380 remains separate. No profitability or scaling readiness is established.

The new [primary polling pipeline](../research/current-polls-v1/README.md) retains six hashed sources, actual field/publication/observation clocks, verified general-election ballots and separate Republican/Democratic assessments. Iowa's newer Marist registered-voter result is preserved as conflicting evidence, not silently pooled with likely voters. Three exact subsequent contract/book comparisons qualified zero entries under the existing model assumptions and pricing policy. These comparisons are not fills, profit or a calibrated performance evaluation. Sources and immutable results are in `research/current-polls-v1`; recovery evidence is `data/production-recovery-20261005.json`.

Next work must establish a useful calibrated forecast and subsequent executable economics, resolve the service-lifetime failure, and prepare a concrete bounded continuation. Regenerating unchanged research clocks, loosening gates to force purchases, or extending the expired activation does not establish success.

The following October4 records are historical.

# October4 live Baal pilot

The newest direct human instruction assigns Codex/Baal execution ownership without requiring Daenerys approval. Assignment `human-to-baal-20261004-2141` is recorded in issue5984685670; fresh Codex ACK5984845412 follows complete scoped official cash/positions/orders/fill reconciliation. The rejected Daenerys RELEASE publication is not fabricated or treated as authority. Existing holdings retain their original attribution.

The single collector now runs the checked native settlement executor. Bootstrap completed2026-10-04T22:00:14.755Z, with a fresh scoped official book and account reads. The1 SUSQie/order,10 aggregate,25 cumulative-purchase pilot uses zero fees, separate durable journals and one serialized schema4 queue. Three persistent services remain: collector, file-only paper worker and local Next.js dashboard. Current source bank has0 qualified forecasts; status is `waiting_for_signal`,0 orders and0 realized P&L. Production execution plumbing is working; profitability is unproven. Historical official net−0.380 and tiny counterfactual+0.020 realized remain separate.

Primary-source historical election baseline V1 is published under `research/election-baseline-v1`. It predicted73/79 screened historical race directions correctly, with10.04point margin RMSE. That is outcome evidence, not executable market profit or unconditional2026 party-win calibration. Current drafts retain publication clocks, unvalidated generic-ballot transport and third-party limitations; no forecast was qualified to force a trade. Next work addresses unconditional opposing-candidate outcomes, current ballots and executable pricing, then preserves attribution through source-bank transitions. Live proof is `data/human-pilot-live-20261004.json`; current ownership is `coordination/execution_state.json`.

Earlier records below are historical where ownership, activation or next steps conflict with this update.

# Current state

## Latest historical account observation: exit canceled, October 3, 08:44 UTC

Covered AK sell **4561585** is terminal with **10,000 canceled and zero observed fills**. At **08:44:08 UTC**, all 10,000 remained and AK holdings were 20,000. The parent clicked cancel **exactly once at 08:44:18**. At **08:44:31**, the official portfolio explicitly said **“You have no open orders”**, no cancel buttons remained, and AK still showed 20,000. No fill or execution ambiguity was observed. Effective server-cancel time was not exposed.

The **08:45 review/cancel deadline was satisfied** before it arrived. There is no remaining order-specific cancellation task. RI YES 19,621, MA YES 2,061 and GA NO 10,000 were unchanged in the parent's terminal portfolio observation. No other order remains open as of 08:44:31.

The **21-row executed ledger is unchanged byte-for-byte**: this cancellation creates no sale row, proceeds, inventory reduction or realized gain. Recorded-price cash remains **65,571.410**, acquisition basis **34,585.125**, and analytical realized P&L **156.535**. No fresh cash display was supplied; the earlier 65,571.41 UI observation retains its original provenance and is not relabeled an 08:44 measurement. Exact hidden balance/fees remain unverified.

The prior accepted-order audit and all earlier snapshots remain immutable. New records: [terminal cancellation audit](../data/orders/daenerys_2026-10-03_ak369_yes_sell_001_canceled_0844.json), [historical portfolio](../data/snapshots/portfolio_2026-10-03_0844.json), [reconciliation](../data/snapshots/reconciliation_2026-10-03_0844.json). [Issue1 terminal notice](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5967306275) notifies Drogon; execution ownership is unchanged. No account calls were made by this record worker.

## Latest historical update: exact AK fills and covered exit, October 3, 08:14 UTC

Fresh parent UI observation at **08:14:34 UTC** confirmed **20,000 AK369 YES held** and all **10,000 shares remaining** on covered sell **4561585**. The sell was submitted once **08:04:20**, accepted **08:04:56**, and its market own-order row confirmed exact limit **0.365 at 08:05:25**. Portfolio's **0.36** is rounded. **No exit fill, proceeds or realized gain is recorded**. Parent review/cancel target: **08:45 UTC**.

### Published curated source now supports both GA and AK fill products

The independently inspected [selected curated projection excerpt](../data/collector-evidence/ga-ak-projection-20261003-0815.json), published in commit **d08dee0570ef3ce0af845f51ae9b4c4d0bd22205**, contains matching closed BUY order fields and selected GA/AK fills. Excerpt SHA-256 is **bffceaec8d6c0203124f1c460de2fae2cbde02894f02c7f6368d4576ad460f62**. Its separately reported full source-projection hash is **aca347f99493165d132c8d587d606a3ba9a732c939138cc196bc1895d0f0d209**; this worker did not inspect or rehash those full projection bytes.

Excerpt capture: **08:13:15.145**; fills observed **08:12:38.737**; order history observed **08:12:03.392 UTC**. These differ from the earlier relayed **08:07:20.306** observation and are evidence clocks, not newer account holdings. **accountHistoryHasMore=true**; global account pagination is incomplete. Selected unique fills sum to each selected closed BUY quantity. This is a curated API projection excerpt, **not original HTTP bodies/headers or fee proof**.

AK BUY order **4547722 / exchange1058** contains five fills, all at projected execution time **07:57:16.338 UTC**:

- **18780665:**16,555 at 0.295 = **4,883.725**
- **18780660:**1,408 at 0.290 = **408.320**
- **18780655:**20 at 0.290 = **5.800**
- **18780650:**1,403 at 0.290 = **406.870**
- **18780645:**614 at 0.280 = **171.920**

Total **20,000 / 5,876.635**; exact weighted average **0.29383175**. Decimal, rational and integer-milli arithmetic agree. The observed transaction5,876.63 and ticket/mark-derived5,876.64 are each0.005 away from this source-supported principal; their original records remain intact and no platform rounding or fee rule is inferred. GA order4505792's matching identity fields and four fills also match the earlier reported **8,945.420** principal.

### Accounting boundary

Ledger **row21** records the terminal AK buy, preserving all 20 prior rows byte-for-byte. The old separate AK observed-cost layer is retired; the already-held20,000 shares are not acquired or debited again. Ledger-only cash **65,571.790** plus the unchanged bot adjustment**−0.380** gives recorded-price cash **65,571.410**. Prior UI cash**65,571.41** agrees at displayed precision, but hidden cash/fees remain unverified and no new08:14 cash reading is claimed.

Total acquisition basis is **34,585.125**; cash plus basis **100,156.535**; analytical realized remains **156.535**. The sell stays outside the executed ledger and does not reduce holdings or add3,650 potential proceeds. Other holdings' older observation timestamps remain unchanged. The exact new source is historical evidence, not a new account-state query by this worker.

Records: [AK exact-buy audit](../data/orders/daenerys_2026-10-03_ak369_yes_buy_001_exact_0815.json), [covered-sell observations](../data/orders/daenerys_2026-10-03_ak369_yes_sell_001_open_0814.json), [reconciliation](../data/snapshots/reconciliation_2026-10-03_0814.json), [portfolio snapshot](../data/snapshots/portfolio_2026-10-03_0814.json). Parent [execution update](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5967087580); [prepared sell](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5967002066). Earlier observations and their original uncertainty remain preserved below.

## Latest historical account observation: Alaska entry, October 3, 07:57–07:58 UTC

The parent submitted **one BUY20,000 YES** for **Republican Alaska Governor market369**, limit **0.295**, maximum principal **5,900.000**, at **07:57:16 UTC**. The full **20,000-share holding** and **zero open orders** were confirmed at **07:58:02**; history was inspected at **07:58:37**. The completed order/exchange IDs and exact fill breakdown were not exposed. This is a new Daenerys position, separate from the earlier closed bot roundtrips in market369.

### Preserve the display discrepancy; do not invent exact cost

- Transaction history: **20,000 YES @ rounded0.294**, displayed amount **−5,876.63**
- Earlier ticket estimate: **5,876.64**
- Portfolio marked value **5,410.56** and displayed unrealized P&L **−466.08** imply **5,876.64** by subtraction, which is still mark/display arithmetic, not a fill receipt
- Cash ticket after the entry: **65,571.41**; its exact observation timestamp and hidden precision were not supplied

Neither displayed amount resolves exact execution cost. No half-cent value, midpoint, fee or fill allocation is inferred. **20,000×rounded0.294=5,880.000 is not booked as actual cost.** The completed holding is added through a separate observed-precision acquisition record; the exact 20-row ledger remains byte-for-byte unchanged pending fill evidence.

Recorded pre-AK cash arithmetic **71,448.045** gives two **display-only bridges**: subtracting history5,876.63 gives **65,571.415**; subtracting estimate/mark-derived5,876.64 gives **65,571.405**. Neither is claimed as exact current cash. Known RI/MA/GA recorded basis is **28,708.490**, including peer-reported GA fills. Exact AK cost, current total basis and current recorded-price cash remain null. Analytical realized P&L remains **156.535** because no new sale occurred.

Existing RI19,621, MA2,061 and GA NO10,000 are carried forward from their earlier records, with their actual older observation timestamps retained. No new independent observation of those holdings is asserted here. The AK20,000 plus those recorded holdings total51,682 shares. Full-position liquidation value remains unknown.

The former **08:15 remainder review/cancel target is inactive** because all20,000 filled and zero orders were verified. Exact AK evidence is already requested from the existing bounded collector; no duplicate request or additional account client was created by this worker.

Sources: [prepared intent](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966948773), [outcome and existing-source request](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966987423), [immutable acquisition audit](../data/orders/daenerys_2026-10-03_ak369_yes_buy_001_observed_0758.json), [reconciliation](../data/snapshots/reconciliation_2026-10-03_0758.json), [portfolio snapshot](../data/snapshots/portfolio_2026-10-03_0758.json). All prior records and historical text remain unchanged below.

## GA exact-fill enrichment: 07:51 projection, account state still as of 07:40

The parent relayed **Drogon's authorized read-only projection** from `official-account.json`, observed **2026-10-03 07:51:34.012 UTC**, reporting `hasMore=false`. This is explicitly **peer-reported evidence**. This record worker has not independently inspected the original raw API response/projection file; sanitized source proof has been requested. The projection time does **not** establish fresh post-07:40 account holdings or open orders.

Reported GA market **259 NO**, order **4505792**, exchange **948**, contains four fills, all with reported API execution time **07:39:38.324 UTC**:

- Fill **18596657**: **20 at 0.880**, cost **17.600**
- Fill **18596663**: **410 at 0.885**, cost **362.850**
- Fill **18596668**: **36 at 0.890**, cost **32.040**
- Fill **18596674**: **9,534 at 0.895**, cost **8,532.930**

Decimal, independent rational and integer-milli arithmetic agree: **10,000 shares**, cost **8,945.420**, exact weighted average **0.894542**. This matches the original UI/export amount **8,945.42** and explains why its rounded average **0.895** must not be multiplied to assert an **8,950** execution cost. Fees and hidden precision are still not inferred. These fills belong to Daenerys's previously observed order, not Codex bot-attributed inventory. The parent's approximate click at **07:39:38** remains separate from the reported API execution timestamp.

### Ledger integration without another debit

The terminal GA aggregate is now **ledger row 20**, preserving the original 19-row byte prefix. Its prior separate observed-precision acquisition layer is retired. This adds **no new holding or cash debit** relative to the earlier accounting bridge: it enriches the same already-held 10,000 shares with reported fill evidence.

Ledger-only cash arithmetic is **71,448.425**; the separate bot adjustment **−0.380** yields **71,448.045**. This is exact arithmetic on recorded numbers, including peer-reported GA prices, **not verified hidden account cash**. The observed UI remains **71,448.04**. Acquisition basis is RI **17,763.900**, MA **1,999.170**, GA **8,945.420**, total **28,708.490**. Cash arithmetic plus basis is **100,156.535**; analytical realized P&L remains **156.535**.

The derived view uses `cash_arithmetic_from_recorded_qty_price` for the enriched value and explicitly retains the peer-source limitation. The older “verified” price-input field remains null rather than suggesting independent raw-API verification. Holdings **RI19,621 / MA2,061 / GA NO10,000**, and zero open orders, still refer to the parent's **07:40:09** observation; history verification was **07:40:17**. Numeric exchange948 applies to the reported GA order only.

Sources: [parent receipt of peer report](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966940428), [original reconciliation request](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966866303), [immutable peer-fill transcription](../data/orders/daenerys_2026-10-03_ga259_no_buy_001_peer_fills_0751.json), [reconciliation](../data/snapshots/reconciliation_2026-10-03_0740_enriched_0751.json), [enriched historical snapshot](../data/snapshots/portfolio_2026-10-03_0740_enriched_0751.json). Original observations, limited-precision audit and official CSV remain unchanged below and in their immutable files.

## Latest historical account observation: October 3, 07:38–07:40 UTC

The parent operator's official signed-in website observations establish **zero open orders at 07:40:09 UTC**, with **RI YES 19,621**, **MA YES 2,061**, and **GA market 259 NO 10,000**. The latest history confirmation was **07:40:17**. This is historical state, not a later live-state assertion.

### Rhode Island terminal reconciliation

Order **4482453** is terminal: **12,621 RI YES filled at 0.900**, exact displayed quantity × price cost **11,358.900**; **2,379 canceled** after one cancel click at **07:38:32**. Zero orders and unchanged RI 19,621 were verified **07:38:54 UTC**. Exact execution and server-cancel times remain unknown.

The ledger appends this as **row 19**, preserving all original 18 rows byte-for-byte. The old separate 12,621-share partial-fill adjustment is retired from the derived accounting: **moving the same fill into the ledger creates no new debit or holding**. RI basis remains **17,763.900** and MA **1,999.170**. Ledger-only cash is **80,393.845**; after the preserved bot adjustment **−0.380**, known pre-GA arithmetic is still **80,393.465**.

### Georgia acquisition: filled quantity verified, exact cost unresolved

New intent **daenerys-20261003-ga259-no-buy-001** requested BUY **10,000 NO** in **Republican Georgia Senate market 259**, limit **0.895**, principal cap **8,950.000**. One click at **07:39:38** was followed by the full 10,000-share holding and zero open orders at **07:40:09**. History at **07:40:17** showed displayed average **0.895** and aggregate cost **8,945.42**. The completed order ID and underlying fills were not exposed.

The official CSV downloaded at approximately **07:41 UTC** repeats **10000 NO @ 0.895 / −8945.42**. The three-decimal average is rounded: **10,000 × 0.895 = 8,950.000 is not the observed acquisition amount**. The **4.580** difference is not labeled a fee, and no fill allocation is invented. The two-decimal 8,945.42 display also does not establish hidden execution precision.

GA is retained as a **completed holding in a separate observed-precision acquisition record**, not an open order or unsupported exact ledger aggregate. Current exact GA cost, exact total basis and exact post-GA price-product cash therefore remain **null** in the derived view. A separately labeled arithmetic bridge is **80,393.465 − displayed 8,945.42 = 71,448.045**, compared with observed UI cash **71,448.04**. That bridge is not an asserted exact cash balance. Known RI/MA basis is **19,763.070**; adding the same displayed GA amount gives a mixed-precision basis bridge **28,708.490**. Their sum cancels the provisional amount and is not evidence of its precision.

Analytical realized P&L remains **156.535**; these acquisitions created no realized sales. No exact new settlement endpoint or whole-account liquidation gain is claimed. Exact GA fills/order ID have been requested through the existing bounded read-only collector; they will be reconciled in a separate later record without rewriting this observation or double-counting GA.

Sources: [GA prepared intent](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966825237), [reported outcomes](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966858454), [exact-data request](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966866303). Immutable records: [RI terminal audit](../data/orders/daenerys_2026-10-03_ri387_buy_002_terminal_0738.json), [GA observed-precision audit](../data/orders/daenerys_2026-10-03_ga259_no_buy_001_observed_0740.json), [official CSV bytes](../data/exports/transactions_2026-10-03_0741.csv), [reconciliation](../data/snapshots/reconciliation_2026-10-03_0740.json) and [portfolio snapshot](../data/snapshots/portfolio_2026-10-03_0740.json).

The supplied CSV contains a limited set of displayed transactions; it is not asserted to be the full account history. Its exact original bytes are preserved, including its final newline state, with SHA-256 **0f67bfe1ba38e649b7553e1a7896bb9568434be7aa9bec1f165ae813c6b8c5d1**. It contains only contest transaction columns and no observed secrets. Prior historical blocks remain unchanged below.

## Latest historical account observation: October 3, 07:23–07:30 UTC

This timestamped update supersedes only the earlier portfolio observations below. The parent operator supplied the official signed-in website observations; this worker did not access the account. All previous audit/snapshot files remain immutable.

- **Order 4419984 is terminal:** 2,000 RI YES bought at **0.890**, cost **1,780.000**; 1,000 unfilled shares canceled. Fresh holdings/order observation was **07:23:20**; one cancel click at **07:23:31**; explicit zero open orders and the 2,000-share history fill verified at **07:23:58 UTC**. Exact fill and server-cancel times remain unknown
- Browser-control timeout interrupted observation approximately **07:12–07:22**. No order actions occurred in that gap. Reset recovered the same signed-in browser without new authentication
- The old order added RI holdings to **7,000**, basis **6,405.000**, displayed average **0.915**. UI cash **91,752.37** is separate from arithmetic **91,752.365**
- The parent recorded a distinct larger intent after the user's explicit **07:10** tournament-upside instruction: **daenerys-20261003-ri387-buy-002**, BUY **15,000 RI YES at limit 0.900**, maximum principal **13,500.000**. Fresh zero open orders at **07:29:19**, then one submission click at **07:29:27 UTC**
- At **07:29:56 UTC**, order **4482453** had **2,379 remaining** and RI holdings were **19,621**. History at **07:30:18** confirmed **12,621 bought at 0.900**, cost **11,358.900**. No later fill is assumed. Review/cancel remainder target: **07:40 UTC**

### Exact combined accounting and ledger boundary

The append-only ledger now has **18 executed order aggregates**: only terminal order 4419984's **2,000 executed shares** were added; all original 17 rows remain byte-for-byte unchanged. Ledger-only cash is **91,752.745**. Apply the preserved separate bot adjustment **−0.380**, then the open order's already confirmed fill cost **−11,358.900**, to obtain combined cash **80,393.465**. UI cash was **80,393.46 at 07:29:42**, a separate two-decimal observation; no hidden rounding method or fee is invented.

Order 4482453's confirmed 12,621 shares are already counted in cash and holdings through a **separate partial-fill adjustment**, although its terminal aggregate is not yet in the ledger. When a terminal aggregate is later recorded, retire this adjustment rather than deducting its cost twice. The unfilled 2,379 shares create no holding or executed cost.

RI basis is **17,763.900 for 19,621 shares**; displayed average **0.905** is not substituted for the exact basis ratio **17,763.900 / 19,621**. MA stays **2,061 shares**, basis **1,999.170**. Combined basis **19,763.070** plus cash arithmetic **80,393.465** equals **100,156.535**. Analytical realized P&L remains **156.535**; buys add no realized gain. Whole-account executable liquidation value is unknown.

If all remaining 2,379 fill at the limit, the additional cost would be **2,141.100** and RI would reach **22,000 shares / 19,905.000 principal**, about **19.9% of initial capital**. This is a risk scenario, not an observed fill. Numeric exchange ID, exact execution times, hidden precision and fees remain unexposed; the selected website context is Midterm Elections.

Sources and immutable records: [terminal source comment](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966750966), [new intent](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966767946), [terminal order audit](../data/orders/daenerys_2026-10-03_ri387_buy_001_terminal_0723.json), [partial order audit](../data/orders/daenerys_2026-10-03_ri387_buy_002_partial_0730.json), [exact reconciliation](../data/snapshots/reconciliation_2026-10-03_0730.json), and [immutable portfolio snapshot](../data/snapshots/portfolio_2026-10-03_0730.json). The historical blocks below retain their original observation times and are not later live-state claims.

## Latest historical account observation: October 3, 06:55–07:01 UTC

The following new observation supersedes the older account summary below only for its stated times. Current ownership is unchanged. The source is **the parent operator's observed official signed-in UI**, transcribed into a sanitized immutable record; this is not raw API evidence or an independently inspected screenshot.

- Before the new order, UI cash displayed **93,532.37 SUSQies** to two decimals. Exact cash-observation time within approximately 06:55–07:01 and hidden cash precision were not exposed; no post-acceptance cash display is asserted
- Holdings were **RI market 387 YES 5,000 at displayed average 0.925** and **MA market 379 YES 2,061 at 0.970**, unchanged at **07:01:51 UTC**. Historical remaining FIFO costs stay **4,625.000** and **1,999.170**, total **6,624.170**
- Account-wide zero open orders was observed at approximately **06:55:40** and at **07:01:07 UTC**
- At **07:01:51 UTC**, the account had one accepted resting order: **4419984**, intent **daenerys-20261003-ri387-buy-001**, **BUY 3,000 RI YES at limit 0.890**, remaining **3,000**, **zero fills observed**. One submission click was made approximately **07:01:16 UTC**, with no retry. Principal cap **2,670.000**; review/cancel target **07:30 UTC**. This is not an executed ledger trade
- Execution context was explicitly selected **Midterm Elections** on the official website; the numeric exchange ID was not exposed and remains null

### Separate post-ledger fill adjustment

The parent observed **12 additional displayed fill rows**, all dated **Oct 2, 2026** in the UI, across AK Republican Governor 369, VT Republican Governor 313, MA Democratic Governor 370 and FL Democratic Senate 354. The supplied reverse-chronological row order is preserved and separately replayed oldest-first. Exact execution times, fill IDs, order IDs and order grouping remain unknown: **12 fill rows are not asserted to be 12 orders**.

Exact Decimal products give purchases **21.080**, sale proceeds **20.700**, and net cash/analytical realized change **−0.380 SUSQies**, with **zero ending adjustment inventory**. Market changes are AK 369 **+0.080**, VT 313 **−0.140**, MA 370 **−0.360**, FL 354 **+0.040**. Parent-reported bot attribution is not independent order-ID attribution.

The original **17-row executed-order aggregate ledger remains byte-for-byte unchanged**. A separate immutable adjustment layer bridges its **93,532.745** cash arithmetic to **93,532.365** and analytical realized **156.915** to **156.535**. The ledger alone therefore does not represent the latest combined cash. The **93,532.37** display is **0.005 above** combined arithmetic; no hidden balance, rounding convention or fee is invented. Cash arithmetic plus unchanged position basis equals **100,156.535**, an accounting identity, not liquidation value. Future reconciliation must avoid double-counting this adjustment if underlying orders are later identified.

### Marks, audit links and limits

At approximately **06:55 UTC**, holdings were marked **6,451.68** and rounded account value was **99,984**. These are provisional valuation marks. Current whole-account executable liquidation value is unknown. The unfilled order is not deducted from cash or counted as acquired inventory; cash reservation behavior was not observed.

Sources: [pretrade observation](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966556594), [intent](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966573510), [acceptance](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966585592). Records: [immutable UI transcription](../data/snapshots/takeover_observations_2026-10-03_0655_0701.json), [exact adjustment reconciliation](../data/snapshots/bot_adjustment_reconciliation_2026-10-03_0701.json), [pending-order acceptance](../data/orders/daenerys_2026-10-03_ri387_buy_001_accepted.json), [immutable derived portfolio](../data/snapshots/portfolio_2026-10-03_0701.json), and [derived latest view](../data/portfolio/latest.json).

Validation replays the original 17 rows with Decimal and independent rational FIFO, replays all 12 additional rows with exact Decimal products and independent rational net cash, checks no oversells and zero adjustment inventory, and verifies unchanged baseline hashes. All original source audits/snapshots and the ledger are retained. The documentation below is preserved as historical context, including its older times and arithmetic. No account API, UI, order or cancellation action was performed by the record-maintenance worker.

## Preserved October 2 account history and ownership context

**Current ownership, October 3:** Daenerys / dot is the temporary sole execution owner under the human's 06:48:26 UTC instruction and 06:53:24 UTC confirmation that the bot is inactive and the PC is off. [Acknowledgment and scope](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966533944); handoff `human-20261003T064826Z-codex-to-daenerys`. Codex / Drogon must keep account writes and collectors paused, including on restart, until a new coordinated return handoff after the human says the bot is active. Old activations must not resume automatically.

The PC-off/bot-inactive state is human-reported, not a remotely verified process stop or shared-lock/lease proof. This documentation change does not control a runtime. Resting orders may still fill; fresh account reconciliation is required before any action. Prior pilot observations remain historical.

Last portfolio review: **2026-10-02 06:06:14–06:06:57 UTC**. Cash displayed **93,532.74** to two decimal places at **06:06:14**. The terminal portfolio at **06:06:57** confirmed **RI 5,000 YES**, **MA 2,061 YES**, and **zero open orders**. These are historical observations and need fresh reconciliation before any action.

## Account reconciliation

- Starting simulated cash: **100,000.00 SUSQies**
- Observed cash display: **93,532.74 SUSQies**, two decimal places; exact hidden account precision was not exposed
- Separate quantity × price ledger cash: **93,532.745 SUSQies**
- Remaining FIFO position cost: **6,624.170 SUSQies**
- Arithmetic cash plus remaining FIFO cost: **100,156.915 SUSQies**, an accounting identity rather than current liquidation value
- October 2 session realized P&L: **0.000 SUSQies**; cumulative realized P&L: **156.915 SUSQies**, under the recorded analytical FIFO aggregate-lot convention
- Open orders: **none observed at 06:06:57 UTC**
- Arithmetic reconciliation: 98,157.745 − (5,000 × 0.925) = **93,532.745**

The display is 0.005 below ledger arithmetic. Both are retained separately. The UI's hidden balance precision and rounding method are unknown; no fee, rounding adjustment, or hidden balance is invented. In schema 2, `cash_verified` and `cash_exact_verified_at_utc` remain null; explicit `cash_displayed` and `cash_arithmetic_from_verified_qty_price` fields carry the distinct values.

| Remaining market | Side | Shares | FIFO cost | Acquisition price |
|---|---|---:|---:|---:|
| Democratic Massachusetts Senate (379) | YES | 2,061 | 1,999.170 | 0.970 |
| Democratic Rhode Island Senate (387) | YES | 5,000 | 4,625.000 | 0.925 |

FL, AK and NC holdings are zero. MA and RI retain unrealized/liquidation risk; the 156.915 realized gain is not whole-account liquidation profit.

Machine-readable historical view: [`data/portfolio/latest.json`](../data/portfolio/latest.json). Immutable [06:06 snapshot](../data/snapshots/portfolio_2026-10-02_0606.json), [full-ledger replay and provenance](../data/snapshots/reconciliation_2026-10-02_0606.json), and append-only [trade ledger](../data/trade_ledger.csv). The prior [22:22 snapshot](../data/snapshots/portfolio_2026-10-01_2222.json) and all earlier snapshots are unchanged.

## Completed October 2 Rhode Island session

Exactly one new executed aggregate is recorded: **BUY 5,000 RI YES at 0.925**, cost **4,625.000 SUSQies**, submitted approximately **05:45:48 UTC** and verified in portfolio transaction history at **05:46:44 UTC**. The completed buy order ID was not exposed and remains null, with a blank ledger order-ID cell. Exact fill execution timestamps are unknown; submission and verification times are not execution times. All **16 prior ledger rows are preserved byte-for-byte**, giving **17 executed aggregates** after the append.

Covered sell **1513474**, **5,000 at 0.955**, filled **zero** shares. One initial request failed with an explicit service-unavailable message; zero open orders was verified before a second attempt was accepted. Cancellation was clicked approximately **06:05:52**, own-order removal was observed by **06:06:14**, and account-wide zero open orders was confirmed at **06:06:57 UTC**. Effective server cancellation time is unknown. The position remains held; no sell row or realized gain is recorded.

The earlier Massachusetts exit **1497695**, **2,061 YES at 0.975**, also filled **zero** shares. Cancellation was clicked **05:10:14**, with terminal zero orders verified **05:11:34 UTC**; MA inventory and basis are unchanged. The [cancellation summary](../data/orders/cancellations_2026-10-02_0606.json) distinguishes the two terminal orders and their actual observation times.

The [RI audit](../data/orders/session_2026-10-02_ri_value.json) and [MA audit](../data/orders/session_2026-10-02_ma_exit.json) are sanitized derived game-observation records. Their original source hashes and separate published-projection hashes are retained in the reconciliation record; original source bytes are unchanged. The [official UNH source](https://scholars.unh.edu/survey_center_polls/1009/) and [public Kalshi response observed at 05:41:04 UTC](../data/snapshots/ri_public_book_2026-10-02_0540.json) provide research context. Those are external references with settlement and depth limitations, not risk-free cross-venue arbitrage or booked profit.

### UI marks versus historical liquidation depth

At the final portfolio observation, RI marked value was **4,604.50** with displayed unrealized P&L **−20.51**; MA marked value was **1,975.37** with displayed unrealized P&L **−23.80**. These are UI marks, not fills. RI marked value minus recorded basis is separately **−20.500**, leaving a 0.010 discrepancy with its displayed P&L; no fee or precision rule is inferred. The holdings total was **6,579.87**; cash-display plus holdings arithmetic was **100,112.61**, while the rounded account display was **100,113**.

At **06:06:14 UTC**, RI's displayed bid was **0.900 × 40,855 shares**, sufficient depth for the 5,000-share position. This supports a historical **4,500.000 gross liquidation scenario**, or **−125.000** against the **4,625.000** basis, before fees, latency or quote movement. **No sale at 0.900 occurred.** A contemporaneous MA executable book was not captured in the final review, so whole-account liquidation value is unknown.

## Earlier historical sessions

The following session descriptions retain their original observation windows and accounting; they are not the current portfolio.

## Completed 22:19 Florida-exit session

The [immutable source audit](../data/orders/session_2026-10-01_2219.json) is preserved byte-for-byte. One limit sell was submitted at **22:21:19.621 UTC**: **7,500 Florida YES at 0.105**, immediately and completely filled. The transaction history confirmed **787.50** displayed proceeds; exact quantity × price proceeds are **787.500**. The completed order ID was not exposed and remains **null** in the audit, with the ledger order-ID cell blank. Exact execution timestamps and unexposed fill/exchange IDs remain unknown; the submission timestamp is not substituted for execution time.

Preflight found the full 7,500 shares owned and zero open orders. At **22:21:02.949**, the bid was **0.105 for 17,392 shares**, sufficient for this sale. The audit separately records the settled ticket, estimated proceeds and absence of an oversell warning before the one submission. Terminal holdings and transaction history confirmed FL quantity zero, MA 2,061 unchanged and zero open orders. No cancellation was required.

This disposal consumes the final 7,500-share Florida lot acquired at 0.120, for **900.000** FIFO cost. Session realized P&L is **787.500 − 900.000 = −112.500**; cumulative realized P&L is **269.415 − 112.500 = 156.915**. The ledger appends exactly **one executed aggregate** and preserves all 15 prior rows byte-for-byte. Its `cost_susqies` value is **−787.500**, the existing sign convention for sale proceeds. No canceled or unsubmitted intent creates a trade row.

The decision was risk reduction after a materially lower public Florida reference price. The [public-reference research bundle](../research/public_reference_2026-10-01/public_reference_report_2026-10-01.md) retains observation times, depth and settlement differences; these external prices are not guaranteed fair probabilities or arbitrage. The earlier 21:32 Florida order was canceled unfilled; this later executed sale is a distinct order and session.

## Historical sessions

The following descriptions apply only to their original observation windows, not current holdings.

## Completed 21:32 inventory-sale session

The [immutable source audit](../data/orders/session_2026-10-01_2132.json) is preserved byte-for-byte. All five submitted orders are terminal:

| Order | Outcome | Exact verified quantity × price proceeds | FIFO cost disposed | Session realized P&L |
|---|---|---:|---:|---:|
| RI YES sell 1080906 | 6,187 filled at 0.935 | 5,784.845 | 5,684.845 | 100.000 |
| AK NO sell 1082747 | 5,000 filled at 0.330 | 1,650.000 | 1,575.000 | 75.000 |
| NC NO sell 1088818 | 1,538 filled at 0.075 | 115.350 | 99.970 | 15.380 |
| FL YES sell 1084839 | 7,500 at 0.125 canceled unfilled | 0.000 | 0.000 | 0.000 |
| MA YES sell 1086473 | 2,061 at 0.975 canceled unfilled | 0.000 | 0.000 | 0.000 |
| **Session totals** | **12,725 shares sold** | **7,550.195** | **7,359.815** | **190.380** |

The 22:00:00 cancellation target is separate from actual submissions: FL at **22:00:12**, MA at **22:00:36**, once each. Fresh reload at **22:01:18** found no open orders and unchanged MA 2,061 / FL 7,500 holdings. No additional FL/MA fill was observed. The immutable audit retains the actual cancellation times even where its brief notes describe the target hour.

All RI fill-row quantities sum to 6,187, including one combined 469-share row. AK fills are 648 + 20 + 4,332; NC is 1,538. Their exact displayed tick prices and quantities determine proceeds without rounding each fill. The ledger appends exactly three executed order aggregates and excludes the two canceled unfilled orders. It preserves all old bytes, uses negative `cost_susqies` for proceeds, and retains milli-SUSQ precision. No precise execution timestamp or unexposed fill/exchange ID is fabricated.

RI disposal consumes the remaining 1,187-share 0.935 lot (1,109.845 cost) plus the 5,000-share 0.915 lot (4,575.000 cost). AK consumes its remaining 5,000-share 0.315 lot. NC consumes its 1,538-share 0.065 lot. Session FIFO gain is **100.000 + 75.000 + 15.380 = 190.380**; prior cumulative 79.035 + 190.380 = **269.415**. Cumulative per-market totals in the snapshot are replayed from all ledger sales, including the earlier RI at-cost sale. Platform weighted-average basis, tournament marks, and executable liquidation value are separate concepts.

## Earlier historical sessions

The following descriptions apply only to their original observation windows, not current holdings.

## Completed 21:02 discretionary session

The [immutable source audit](../data/orders/session_2026-10-01_2102.json) is preserved exactly as recorded. All four submitted orders are terminal:

- RI YES buy **1056531**: **5,000 at 0.915**, cost **4,575.00**; 3,104 + 1,896 shares filled
- RI YES sell **1050409**: **3,569 of 4,756 at 0.950**, proceeds **3,390.55**; **1,187** unfilled shares canceled. Fills were 1,826 + 282 + 1 + 903 + 1 + 1 + 555 shares
- AK NO sell **1052464**: **5,000 at 0.330**, proceeds **1,650.00**; 1,883 + 224 + 1 + 2,892 shares filled
- FL YES sell **1054809**: **5,000 at 0.125**, proceeds **625.00**

The cancellation target was **21:25:00 UTC**. The one actual RI cancellation was submitted at **21:25:12**. At **21:24:46**, 1,742 shares remained; **555 additional shares filled for 527.25 before cancellation finalized**, leaving 1,187 canceled. The 555 are included once in the 3,569 total. AK and FL were already filled and were not canceled. Zero open orders was verified at 21:26:00 and cash at 21:26:19. This delay and fill/cancel race are retained, not replaced with the target time.

The three superseded, unsubmitted intents (RI buys at 0.920 and 0.840; RI sell at 0.945) created no trades. The ledger appends only the four actual executed aggregates; its `cost_susqies` is positive for buy debits and negative for sale proceeds. Canceled quantities and unsubmitted intents are excluded. Exact execution timestamps, per-fill IDs and exchange IDs were not captured; they remain unknown. Submission and verification times are not execution times.

### Exact FIFO analysis

This is analytical FIFO using the previously documented acquisition lots, not the platform's displayed average basis or interim tournament marks:

| Disposal | Proceeds | FIFO cost disposed | Realized P&L |
|---|---:|---:|---:|
| RI 3,569 YES from 0.935 lot | 3,390.55 | 3,337.015 | 53.535 |
| AK 5,000 NO from original aggregate lot | 1,650.00 | 1,649.50 | 0.50 |
| FL 5,000 YES from 0.120 lot | 625.00 | 600.00 | 25.00 |
| **Total** | **5,665.55** | **5,586.515** | **79.035** |

RI remaining cost is 1,187 × 0.935 + 5,000 × 0.915 = **5,684.845**. Its exact aggregate average is **5,684.845 / 6,187**; the snapshot preserves the ratio rather than substituting a rounded average. AK retains the newer 5,000-share 0.315 lot. The disposed original AK order ID remains unknown. Prior realized P&L was 0.00, so session and cumulative realized P&L are both 79.035. The 79.04 display rounding is not used in reconciliation.

The source audit includes interim observations and the AK `proceeds_verified_by_cash_change` value 695.31. That interim value is not the terminal AK proceeds; the full 5,000-share fill and exact final cash reconcile to **1,650.00**. The original audit is not rewritten to erase intermediate evidence.

## Completed 20:36 discretionary session

[Session audit](../data/orders/session_2026-10-01_2036.json) contains exact observed order IDs, intent IDs, submission/cancellation/verification times and transaction-history fill quantities. All five order intents are terminal:

- RI YES sell **2,072 at 0.965**: fully filled, proceeds **1,999.48**, realized P&L **0.00** against the original lot. Completed sell order ID and exact execution timestamps were not exposed and remain unknown
- RI YES buy order **1023469**: **4,756 of 5,000** filled at **0.935**, cost **4,446.86**; **244** unfilled shares canceled at **20:57:06 UTC**
- AK NO buy order **1026814**: **5,000** filled at **0.315**, cost **1,575.00**. Combined with the original 5,000-share lot costing 1,649.50, remaining AK basis is **3,224.50**
- AK NO sell order **1028485**, 5,000 at 0.335: **unfilled**, canceled at **20:57:19 UTC**
- FL YES sell order **1030028**, 5,000 at 0.130: **unfilled**, canceled at **20:57:39 UTC**

The cancellation target was 20:57 UTC; actual cancellation submission times and the later zero-open-orders observation above are authoritative. No additional fill/cancel race was observed. Each cancellation was submitted once; a transient Florida UI wait did not trigger a duplicate submission.

The ledger adds only three actual trade aggregates, not canceled unfilled quotes. Its existing `cost_susqies` column is signed cash cost: positive for purchase debits, negative for sale proceeds. This permits initial cash minus cumulative signed cost to reconstruct cash. It is not a realized-P&L field. Exact fill timestamps remain unknown; submission and verification times are not substitutes. The source audit preserves the 100+1,972 RI sale fills and 1+3,907+2+1,090 AK purchase fills without inventing execution IDs. Rounded AK fill displays sum to 1,575.00, while exact per-fill products can carry half-cent precision; aggregate accounting uses the exact 5,000 × 0.315 cost.

## Important limitations

The portfolio page uses tournament valuation prices whose formula remains unverified. Its marks differed sharply from detail-page prices. Neither is a guaranteed full-position exit price. Current displayed gains/losses are not realized P&L. No fee or hidden-precision rule is inferred. Realized gain excludes all remaining position risk.

See the [execution and valuation review](../research/execution_valuation_review_2026-10-01.md) for historical depth and observation windows. Quotes were not captured simultaneously and are stale for execution.

Under standard 0-or-1 settlement and no further trades, fees, refunds or sponsor interventions, ledger-arithmetic endpoints for the two remaining MA/RI positions are **93,532.745** if both lose and **100,593.745 SUSQies** if both win. These use the separate arithmetic balance, not an asserted hidden UI balance; they are scenarios, not forecasts or executable valuations.

## Historical operating mode (October 2)

Codex is the sole execution owner under the human's October 2 reassignment. Daenerys remains advisory; the former account worker was confirmed stopped in issue comment 5947535773. The historical portfolio observations above are preserved separately from current bot-attributed activity.

The same small pilot run, `sig-pilot-20261002-0824`, was renewed at **2026-10-02 14:18:24.002 UTC**, with new entries authorized until **20:18:24.002 UTC**. The original authorization is archived. Cumulative accounting was retained: **4.360 SUSQ** of the unchanged 100 SUSQ cumulative purchase ceiling has been consumed. Limits remain 25 SUSQ per order, 100 SUSQ aggregate attributed cost, three markets, 1% displayed depth and zero added fees. The ownership file is a coordination record, not an execution lock.

The collector restarted with the authorization-window correction in commit 8d5c4d4a7a52c82f0702e57ca62398d145a2ce5a. Fresh official account, market and book bootstrap completed at **14:19:11.843 UTC**. At 14:19:54, six attributed orders were reconciled across two completed roundtrips, with no open bot intent or bot-owned inventory and **+0.035 SUSQ realized P&L**. That tiny sample does not establish profitability or justify scaling; it excludes the older unrelated holdings described above. Current runtime observations are displayed in the local dashboard.

BUY authorization is rechecked at preparation and both queued dispatch fences. Owned exits, cancellation and reconciliation remain available outside the entry window; any expired recovery reuses the previously attempted exact key and payload.

## Historical bot work queue (paused)

The following prior work queue is preserved for a future coordinated return; it does not authorize restarting workers, collectors or account writes during Daenerys's temporary ownership.

1. Keep one collector and all existing workers healthy, preserving frozen banks, source journals and cumulative risk limits.
2. Compare the fixed dip/rebound wallets on subsequent live observations; recent quiet price paths have produced no qualifying signals.
3. Test price-protected entry execution prospectively under unchanged discount gates. Two observed official entries caught short-lived ask concessions; this motivates a test, not a profitability claim. A quote touch or depth disappearance is insufficient proof of a resting fill.
4. Monitor actual completed-trade economics and fresh account reconciliation before any scaling or further pilot-window renewal.

# October5: actual collector status writer repaired

At16:38UTC the collector stopped on EPERM while publishing its root status. The earlier bounded Windows retry fix covered ledger projections, but this runner call still used the immediate-rename transport helper. Both derived collector status views now use the durable writer through a JSON-shape-preserving adapter. Sixty-six targeted checks include the actual runner call, transient sharing denial and persistent failure. Recovery verified one collector PID2288, all nine fixed activation/configuration files unchanged, and all eight frozen bank/journal prefixes preserved. This repairs collection; it establishes no strategy profit. Realtime market capture remains separate from official execution.

# October5: actual realtime economics and causal forecast training

Streaming replay processed842,779 original records and25,798 trades in46.7 seconds. Small paired gains were outweighed by one-sided five-minute markouts; the overlapping diagnostic is not a funded profit result. Six causal queue/flow forecasts trained from58,942 supported labels; the selected model improved later prediction error modestly but produced zero opportunities after its predefined margin. It is not active. The collector now reuses its four existing book slots through the guarded depth200 dispatcher to establish realtime baselines without additional reads. Frozen wallets, official activation and account-wide pacing are preserved. See [measured results and implementation](REALTIME_ECONOMICS.md).

