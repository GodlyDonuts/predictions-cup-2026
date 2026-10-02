# Sequential paired paper v3

This independent successor adds executable profit-taking to the scoped NH dual-NO paper experiment. It reads the existing serialized collector; it sends no competition orders and makes no API requests. Older cohorts, code and journals remain intact.

A matched lot can prepare a close only when both fresh, cofresh YES asks imply NO sale proceeds covering that lot's own remaining acquisition basis plus 0.01 SUSQ per paired share, after one adverse tick per sale. Participation remains 1% of observed depth. The expensive acquired-principal leg sells first; a later independent request may sell only the number actually sold on the first leg. Partial sales retain the matched remainder. An unfilled second leg creates a visible bounded orphan and may require a loss-making salvage sale. A quote-qualified close is never counted as completed profit.

Fees are zero. Each side requires a new request starting at least two seconds after its decision; intents expire after 30 seconds. The original 6-share, 6-SUSQ package, 5-SUSQ orphan, 25-SUSQ aggregate and 100-SUSQ cumulative-buy caps remain. Every transition checks cash plus held basis against initial capital plus realized PnL and checks lot quantities/basis against wallet positions. Replay must reconstruct identical cash, state and journal hashes, including completed sales.

Normal settlement value is conditional on the scoped contract relationship and settlement rules. Refunds or overrides can invalidate that benefit. The simulated NO convention has not been established as the current official REST fill-price convention. Synthetic positive and failed-close cases verify implementation, not profitability. Subsequent untouched collector data supplies the prospective evidence.

Run `node --test live-paper/paired-package-paper-v3.test.mjs live-paper/paired-package-watch-v3.test.mjs live-paper/paired-package-read-plan.test.mjs`.
