# Lossless field-aware scenario compression

The native rank lane accepts128 scenario rows. Separately retained publisher views can exhaust that budget even when some rows have identical economic consequences. [The new research transform](../../moloch/tartarus/scenario_coalescing.py) merges rows only when their complete supplied contract payout tags, exception class, and **every supplied opposing-field threshold** agree. Every source model keeps its own weight vector; no source probabilities are averaged or recalibrated.

For portfolios over those contracts, identical payout tags preserve terminal wealth, including lot-dependent principal refunds and fixed cash/debt. Identical field thresholds also preserve strict wins, ties and their weighted scores. Each source model's integer probability mass is summed within equivalent groups exactly. Original row IDs, labels and payout vectors remain in the provenance map. A different field threshold prevents merging even when own-account payouts match.

## Actual retained-model result

The [dated23-contract study](scenario-coalescing-20261018.json) preserves all five existing Kalshi/CTM/operator models and reduces105 rows to74 under three explicitly hypothetical field cases: a dated flat marked anchor, a1.5m growing anchor, and a field correlated with modeled YES outcomes. The complete joint economic distribution is independently compared before/after for each source model. A fourth adversarial field that distinguishes every original row correctly retains all105 rows. These field cases are assumptions, not private rival positions or calibrated terminal-wealth forecasts.

The original source clocks and decision time are retained; compression does not renew expired inputs. This result frees31 rows under the existing native limit. It changes neither the modeled economics nor current profitability and is not an executable campaign.

## Research and execution boundaries

The party compiler keeps128 as its default bound. An explicit `research_scenario_limit` may retain up to4096 research rows for subsequent field-aware reduction. It never drops scenarios or probability models to fit. The coalescer either returns at most128 rows or rejects the matrix. The native64-contract, source, model, funding, ownership and execution limits remain authoritative.

All state-dependent field models must be supplied before compression. Adding contracts, a latent opposing-wealth model, state-dependent execution, or other state-specific data requires rebuilding from the original rows and expanding the equivalence definition. The helper rejects unsupported scenario/model fields. It does not independently establish resolver equivalence, account completeness, source freshness, field completeness or live authority.

Validation:36 focused coalescing/party/account/capacity checks pass. These include exact money/win/tie equality across100 generated portfolios and every source/field pair, one-ppm exceptional mass, distinct exception classes, a513-row/eight-model synthetic research case, and rejection of latent-field distinctions that would exceed the native bound. No account request, order or runtime restart is performed by this research.
