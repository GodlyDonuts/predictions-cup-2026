# Predictions Cup leaderboard and execution review

Observed through the authenticated official cloud-browser UI on October 1, 2026, 19:58–20:01 UTC. Read-only leaderboard/profile inspection. No API calls or additional orders.

## Bottom line

Substantial trading activity is directly visible: an all-time top-six participant had 3,681 trades and a public profile listing 171 markets traded. However, the visible UI does not establish trading speed, turnover, realized profits, or that rapid day-trading caused the leading balances. Today's ranking differs materially from All Time. Large provisional P&L alone is insufficient evidence of an executable strategy.

## All Time snapshot

Source: https://sig.thesuper.market/leaderboards at approximately 19:58:15 UTC, All Time selected. The header described ranking by SUSQies account value, while the displayed amounts had plus signs. There were 446 traders.

| Rank | Username | Displayed trades | Displayed amount |
|---|---|---:|---:|
| 1 | calm-badger-5206 | 158 | +414.5k SUSQies |
| 2 | kind-brook-6861 | 293 | +346.4k |
| 3 | bright-badger-3145 | 109 | +344.1k |
| 4 | noble-birch-8618 | 710 | +230.3k |
| 5 | steady-finch-4087 | 79 | +227.0k |
| 6 | bright-finch-0400 | 3,681 | +182.2k |
| 10 | steady-willow-0544 | 278 | +136.6k |

These are UI amounts, not a claim about cash balances or realized profit. The account-value header and plus-prefixed display are not sufficient to determine the exact score formula.

## Today snapshot

Same source at approximately 20:00:00 UTC, Today selected. The header described P&L earned today.

| Rank | Username | Displayed trades | Displayed amount |
|---|---|---:|---:|
| 1 | steady-willow-0544 | 278 | +136.5k SUSQies |
| 2 | noble-maple-7448 | 17 | +29.3k |
| 3 | noble-comet-5932 | 152 | +25.1k |
| 4 | eager-birch-3507 | 20 | +13.2k |
| 5 | merry-dawn-1104 | 44 | +7.6k |
| 6 | bright-badger-2307 | 1 | +7.4k |
| 7 | eager-cedar-2317 | 978 | +5.7k |
| 8 | lively-grove-5192 | 9 | +3.7k |
| 9 | merry-dawn-9965 | 433 | +3.7k |
| 10 | calm-acorn-8853 | 559 | +3.5k |

The trade-count time window is not specified in the visible UI. The same 278 count appears for steady-willow in both views. Do not label these as daily trade counts without additional documentation. Differences between views also mean the all-time leader's +414.5k should not be presented as verified P&L earned during the first four competition hours. The reason for the view difference remains unresolved.

## Public profile limits

- Leader calm-badger-5206: https://sig.thesuper.market/user/1d1fa8aa-ec3a-4a16-bc85-f4f5e9ffc3d5, inspected 19:58:55–19:59:03 UTC. Joined September 2026. The page explicitly says the user keeps trade history private. No effort was made to bypass that setting.
- High-volume bright-finch-0400: https://sig.thesuper.market/user/10c9ad63-79f9-493f-a571-a478d1312f3c, inspected 19:59:16 UTC. Rank 6; P&L 182,417.2 SUSQies; 171 markets traded. The visible list includes diverse Senate, governor, and House markets. It does not expose sides, quantities, order/fill timestamps, cash, turnover, or realized/unrealized P&L breakdown.

## Official scoring documentation

At 20:01:10 UTC, https://sig.thesuper.market/docs/portfolio-and-leaderboards describes current position value, daily gains/losses, available balance, and competition standings, but gives no explicit mark-price formula or realized/unrealized split.

At 20:01:29 UTC, https://sig.thesuper.market/docs/tournaments-and-organizations confirms trading closes November 4 at noon ET, final rankings are confirmed after every market settles, and final SUSQie balance determines end-of-competition rank. Starting balance is 100,000 and at least one trade is required. Interim leaderboard P&L is therefore not itself a locked final payout.

The overview at https://sig.thesuper.market/docs states trading uses platform currency rather than real money and is locked until the competition opens. It does not resolve the Today/All Time discrepancy.

## Mark-to-market caution from our account

Earlier account observation at 19:46 UTC showed dashboard marks RI YES .743, MA YES .661, FL YES .142, and NC NO .189, despite materially different market-page execution quotes. After the Alaska fill, its dashboard mark was .366 while market-specific trade/holdings display was .330. Treat dashboard/current-value marks as provisional valuation, not available liquidation prices. This is an observed discrepancy; its exact implementation cause is unverified.

The latest no-open-orders checkpoint belongs to 19:53:13 UTC, not this later leaderboard inspection. At that checkpoint transaction history confirmed Alaska market 377 Buy 5,000 NO, debit 1,649.50 SUSQies. Exact available balance had been 92,751.88 in the market form. See order_alaska_2026-10-01_1951.json. No later reconciliation was performed here; a separate account reconciliation is required.

## Governor quotes and settlement, read-only

Full structured ladders: governor_execution_quotes_2026-10-01_1957.json.

- Iowa Democratic governor market 168, https://sig.thesuper.market/markets/168, 19:55:36 UTC. YES asks: 100 at .845, 796 at .850, 692 at .855, 307 at .860. NO best ask .165; complementary YES bid depth implies NO asks 35 at .165, 750 at .175, 485 at .180, 483 at .185, and 1,000 at .195. No position/order.
- Alaska Democratic governor market 368, https://sig.thesuper.market/markets/368, 19:56:40 UTC. YES asks: 1,854 at .755, 331 at .765, 1,073 at .780, 8,349 at .785. NO best ask .255; complementary YES bid depth implies 1,426 at .255, 150 at .265, 323 at .270, 472 at .275, 450 at .280, 250 at .290, and 172 at .300. No position/order.
- Both Info tabs use party affiliation of the winning candidate, with fusion tickets counting every party on the ticket, based on final certified official results. Recounts/legal challenges may delay resolution; sponsor retains amendment discretion. Planned resolution November 4 noon EST. The Alaska rule does not explicitly name the candidate or adjudicate the Democratic/Nonpartisan joint-ticket issue, so a candidate-win model must not be silently equated to this party contract.

## Evidence limits and screenshots

This report is grounded in the official rendered DOM snapshots, not hidden state or private trade history. No leaderboard screenshot was saved with this report. Any separately captured screenshot must be reconciled with the timestamped views above before claiming screenshot corroboration. No screenshot contents have been inferred here.

## Operational implication

Speed may improve capture of transient executable opportunities, but this inspection does not establish bandwidth or VPS location as the bottleneck. Prioritize measured quote age, execution latency, spread/slippage, inventory risk, and realized trade attribution. Do not chase rank or equate trade count with edge. Preserve the single account-reconciliation owner to avoid overlapping polls or contradictory state.
