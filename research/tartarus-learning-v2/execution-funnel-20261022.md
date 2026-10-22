# R94 execution funnel: where the cadence was lost

The retained dispatch prefix from R94 installation is an exact byte prefix of the stopped R94 dispatch copied at the R97 boundary. All original financial fills are unchanged. The file-only audit compares those immutable copies, with no account requests.

Between the boundaries, there were 15 distinct new intents, 15 attempts, 15 acknowledgments and 15 terminal closures. Four distinct orders actually filled; 13 official partial-fill receipts describe those four orders. All 15 intents used the fundamental lane; none used the reviewed monetary/rank lane. There was one cancellation. Another 139 draft quotes were retired with no network attempts; these do not prove 139 eligible missed opportunities.

The 15 acknowledged POSTs began from15:12:12 through16:16:56 UTC. Median spacing between POST request starts was274,306.5ms (4m34s), minimum91,083ms, maximum484,633ms. Their median HTTP request-to-receipt duration was577ms (range463–764ms). Median durable-intent-to-request-start delay was397ms. That final measurement excludes all prior selection, account reads and preparation: it cannot establish low full-cycle latency.

Three of the four filled orders executed close to request start; the maximum first-fill delay was340,346ms. Acknowledgments alone do not prove execution. These small samples cannot establish a general fill probability, causal strategy benefit, or future throughput.

R97 addresses the delayed reviewed-order service and known ordinary BUY/SELL conflicts. The next comparison needs its actual fresh processing, monetary lane admission, between-order spacing and original fills. Merely reaching the same subsecond HTTP latency would not satisfy the human's one-fill/minute requirement.

Evidence: `r94-execution-funnel-20261022.json`; reproducible tool: `tools/report-execution-funnel.py`. Its partial-fill aggregation reconciles the four filled IDs against `r94-final-turnover-20261022.json`. The tool rejects changed original dispatch/fill prefixes and publishes aggregate durations only.

Deep Research was also started in the human-requested ChatGPT browser against checked sourced91a4c79 and the current published execution evidence: <https://chatgpt.com/c/6ada39a7-98b0-83ea-88c1-e9bfa46bc397>. Its report is pending and cannot be treated as verified strategy or implementation authority.
