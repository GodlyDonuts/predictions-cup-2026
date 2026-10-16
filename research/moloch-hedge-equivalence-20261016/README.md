# Covered opposite BUY versus covered SELL

Bounded read-only finding for [task155](https://github.com/GodlyDonuts/predictions-cup-2026/issues/155). **No missed profitable hedge is established by these inputs.** Adding a gross-funded opposite BUY merely to duplicate an available covered SELL would not add edge. No execution source, guard, policy, control or service changed.

## Economics and current source

For the same binary contract with immediate one-unit YES/NO netting, let the YES book be bid `b`, ask `a`. At zero fees, buying NO at `1−b` against owned YES has net cash proceeds `1−(1−b)=b` per share: exactly a covered YES sale at its bid, with the **same original level quantities**. A passive NO BUY at `1−a` is equivalent to a passive covered YES offer at `a`. YES/NO reversed is symmetric. Different fees, absent netting, semantic differences or unavailable liquidity would require a new calculation; none is silently assumed away.

An opposite BUY needs gross cash before its actual fill/netting; a covered SELL needs owned unpledged shares. There is no credit from an unfilled hedge, future payout or provisional mark. These identities do not prove either route fills, bypass self-trade prevention, or promise all displayed depth is accessible. The native actual-fill ledger performs FIFO pair netting in [Wallet::buy_fill_exact](https://github.com/GodlyDonuts/predictions-cup-2026/blob/e5fa7d6f601a9c5ac507d6e17d0f7feb3b7cc9bb/moloch/native/src/lib.rs#L243); this audit observes **zero actual nettings**, not a new venue cash-transition experiment.

Source inspected at `e5fa7d6f`:

- [Native hedge retention](https://github.com/GodlyDonuts/predictions-cup-2026/blob/e5fa7d6f601a9c5ac507d6e17d0f7feb3b7cc9bb/moloch/native/src/engine.rs#L750) requires `retainProfitableHedge:true`, coverage/overshoot checks and fee-adjusted gain. [Replacement hedge selection](https://github.com/GodlyDonuts/predictions-cup-2026/blob/e5fa7d6f601a9c5ac507d6e17d0f7feb3b7cc9bb/moloch/native/src/engine.rs#L1011) requires `repriceProfitableHedge:true`. The retained policy has **retention false**, replacement flag absent, overshoot0, zero fees,60-second execution-age limit and fixed gain .005. The policy extraction explicitly distinguishes historical seed capital/breadth from later genuine capacity/contribution events.
- [Fixed-gain SELL](https://github.com/GodlyDonuts/predictions-cup-2026/blob/e5fa7d6f601a9c5ac507d6e17d0f7feb3b7cc9bb/moloch/native/src/engine.rs#L1061) uses the most expensive remaining lot plus gain/fees. Where the proposed passive pair clears that same worst-basis threshold, its implied held-side offer may be equal to or less competitive than the existing fixed-gain SELL. A maximum-age/profitable-age [executable exit](https://github.com/GodlyDonuts/predictions-cup-2026/blob/e5fa7d6f601a9c5ac507d6e17d0f7feb3b7cc9bb/moloch/native/src/engine.rs#L982) uses actual bid and the separate configured depth haircut; that quantity restriction is not additional opposite-BUY alpha.
- [Quote priority](https://github.com/GodlyDonuts/predictions-cup-2026/blob/e5fa7d6f601a9c5ac507d6e17d0f7feb3b7cc9bb/moloch/live-runtime/src/quote_priority.rs#L46) and [physical authority](https://github.com/GodlyDonuts/predictions-cup-2026/blob/e5fa7d6f601a9c5ac507d6e17d0f7feb3b7cc9bb/moloch/live-runtime/src/authority.rs#L180) restrict ordinary crossing BUYs. This is a structural consideration for a future separately approved hedge route, **not proof of a present missed gain**. Current source-only flags cannot amend the immutable funded-bank policy. Existing account/unknown/freshness/ownership checks remain necessary for either route.

## What the originals show

The source report ends **2026-10-16T13:35:02.604Z**, bank sequence260738. Its seal, raw file pins and all10,837 original dispatch row seals/chain links verify. Replay of observed fill quantities from original opening holdings matches ending native positions; this is not a second financial-engine or cash/basis replay.

| Finite-prefix observation | Count |
| --- | ---: |
|Unique actual fills|163|
|Ordinary resting-after-ACK BUY fills /shares /intents|50 /20,584 /24|
|Intents with unique same-created complementary native quote|22|
|Those mates whose limit plus first actual entry price is below1|22|
|Mates ever physically attempted /never attempted in this prefix|16 /6|
|Mate cancellation exactly first native fill observation+3seconds|5|
|Whole never-attempted mate covered by its first fill, still unexpired, cancelled3seconds later|0|
|Actual opposite-BUY netting fills/shares, all origins|0 /0|

Eight mate cancellations became effective before the first native fill observation, eight later by other delays, and one mate had already expired. These are recorded-clock relationships. They do not independently prove why a cancellation occurred, that the whole mate was eligible at a later partial fill, or that a never-attempted order would have filled. No first-fill coverage count is promoted into a full historical eligibility replay.

The supplied sealed v7 gzip contains9,720 derived rows:3,883 decision features and5,837 horizon labels, decision window **12:01:37.211–12:36:07.245UTC**. There are **zero matching decision/exchange/side observations with any ordinary inventory known on the report's native observation timeline**. This is missing overlap, not evidence that the market offered no opportunity. The v7 SIG fields contain BBO prices/quantities, source/receipt clocks and referenced journal hashes/sequences; they do not carry a complete original physical preflight or full depth. Its wrapper/decoded hashes are pinned, but this file has no independently verified own row chain or reverified referenced-source membership in this audit.

## Reproduce and next measurement

Private originals remain outside Git. [report.json](report.json) publishes aggregates, input/code hashes, original cutoff and explicit verification limits; it omits own order/fill IDs and account/profile data. Important raw pins:

| Input | SHA256 |
| --- | --- |
|Original report|`93ec8735e455e26bc2d6144eae67cc805d84ac40ebd9820c70605f53adda0f5f`|
|Original dispatch|`07052ce42f7ebfaf6b5b288614fc3fd65d5da72cd756519fc56991df11ba774d`|
|Original pricing-policy extraction|`a2664a76823975c43575b5e88c8ba07fce7241e28c1d68d6f88de26c09182597`|
|v7 gzip wrapper|`3fd8f87f38a93e180d572b5f6f41f672032611fdf5c689737e0f9ad02ce6af0b`|

```text
python -B research/moloch-hedge-equivalence-20261016/reproduce.py \
  --input-directory PRIVATE_MAKER_DIRECTORY \
  --manifest-sha256 dff429da9ae6154679842e1c0458fa30ff053162c35ffb7687a805049322a97a \
  --sealed-v7 PRIVATE_V7_GZIP \
  --sealed-v7-sha256 3fd8f87f38a93e180d572b5f6f41f672032611fdf5c689737e0f9ad02ce6af0b \
  --output NEW_AGGREGATE.json
```

The script reuses the existing bounded raw-seal/fill parser, rechecks all input pins after reading, and bounds v7 decoding to128MiB/100,000rows. Local reproduction completed in under one second; source-pin rejection and zero-fee algebra checks are recorded at handoff. There were no network/account calls, native replay, compiler or service operations. Raw source-code hashes describe this checkout's bytes; LF/CRLF changes can change those hashes without changing parsed code/results.

The next useful record is **one actual decision-time comparison** for an owned cohort: exact FIFO basis and available shares after SELL/netting pledges; original source/receipt/native-decision clocks and full executable levels; equivalent opposite-BUY and covered-SELL price/quantity/fees; BUY gross cash funding; current native qids, original intent/attempt/unknown status; age/deadline; and the first specific eligibility failure. Keep observed book eligibility separate from actual ACKs/fills. A future change needs an observed economically distinct missed execution, rather than a longer TTL or a duplicate gross-funded route.
