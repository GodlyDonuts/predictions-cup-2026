# Static captured-depth rotation capacity

The full retained journal scan at04:42UTC changes the portfolio search. Its573129 original rows contain203 last-seen exchange books, but166 were subsequently invalidated by a native gap event. The remaining37 are dated observations, not a fresh or complete339-market opportunity set. Original prefix length is1,894,800,574bytes; [source pins and exact results](full-book-capacity-20261018.json) retain its SHA256 and source financial time04:16:44.848UTC. No account request was made.

| Captured static opportunity set | Optimistic terminal wealth ceiling |
| --- | ---: |
| Spend original cash; no further sales | 266,887.384615… |
| Model sales of original held shares, then reinvest proceeds | 1,078,519.809523… |
| Dated public leader P&L plus assumed100,000 initial allotment | 1,081,241.680000 |

These ceilings relax fees, debt, reservations, mandate limits, integer quantities, market eligibility, order size, pacing, shared liquidity and conflicting outcomes. Every retained and newly bought binary share is assumed to pay one. This is especially generous when opposite parties or mutually exclusive outcomes occur. The cheapest non-invalidated captured ask is.05; genuine half-cent depth is not established by this scan. These are capacity calculations, not attainable portfolios or expected returns.

The [exact fractional implementation](../../moloch/tartarus/rotation_capacity.py) maximizes this static relaxation. It spends initial cash at increasing ask prices, then pairs remaining asks with decreasing bids on original owned shares. A modeled sale gives up its terminal share payout exactly once. Funding from that sale only improves this bound when its bid exceeds the replacement ask. Unexecuted proceeds never enter an actual account. Six focused checks include independent integer enumeration, scarce depth, no available cash, unfavorable rotations and original input preservation.

The published implementation independently reproduces both original census results from3,594 buy-depth levels and35 sale-depth levels clipped to actual original holdings. Source SHA256 at verification: `96e583f452b72929bbfd68f6128d38ab600479aae7eef11b03b9f83a2ce82ef79`. This verifies the arithmetic and supplied opportunity set; it does not certify source freshness or economic assumptions.

The static captured-book rotation ceiling is2,721.870476… below the indicative marked leader anchor. It therefore supplies no winning static candidate, even under these favorable assumptions. This is not a global impossibility result: missing/invalidated markets, future depth, repeated trading gains, interim settlements and funding changes are outside this calculation. The leaderboard anchor is a dated marked-value inference, not an opposing terminal-wealth forecast.

The next campaign search must expand genuine executable coverage or demonstrate source-qualified sequential turnover. It must retain current cash, actual terminal liabilities, sale-confirmation dependencies, original FIFO, partial-fill and failed-replacement branches. Lowering the target or recycling stale quotes would not establish a winning campaign. The checked R41 production process remains in original-ledger restoration at the last04:44:57UTC process inspection; no actual Tartarus rank fill or ROI is established here.
