# Entry protection versus turnover

Optional `entryBackoffTicks` lowers both flat-entry purchase limits by zero to ten ticks. It cannot be combined with inside-spread improvement. Invalid boundary prices skip the market. Existing funded quotes retain their original limits; covered inventory exits and age controls remain unchanged. The option is absent from the default policy and is not enabled in the current forward wallets.

`node moloch/research/entry-backoff-20261014.mjs NEW_OUTPUT` verifies terminal source snapshots, predecessor links, journal chains and exact baseline financial parity for episodes 18–25. Each variant carries the same inherited wallet continuously rather than resetting capital between episodes. The historical periods have been inspected previously and are development data.

The baseline added five fills and ended with 2.350 owned cost, four unresolved orders and last-fresh liquidation P&L −0.050. One-, two- and four-tick backoff variants added no fills and retained the inherited +0.025 realized gain, no inventory and five unresolved quote liabilities. They generated no new profit. Better marked performance arose from avoiding purchases, not a demonstrated profitable market-making cycle. Backoff alone therefore does not solve the turnover objective and is not promoted into the live policy.

All 121 core Node tests pass. New tests cover both purchase limits, strict through-price fill eligibility, legal price boundaries, incompatible policy rejection and exact zero-backoff financial parity. The prior two directional evaluator tests remain separate. Checked evidence: `data/moloch-entry-backoff-20261014.json`.
