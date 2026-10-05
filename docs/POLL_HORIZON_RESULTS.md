# Horizon and correlated polling error

The generic Senate model now aggregates at29 days to election, with a30-day source-age window and21-day recency half-life. It retains only the latest median-field-date observation per pollster and one equally weighted race in calibration. This is a standalone candidate; the45-day baseline and all active wallets remain frozen.

The verified final-outcome/polling sources supply240 comparable races across13 election cycles. Historical margin-error SD is7.596 points. The estimated common-cycle variance contributes a0.124 correlation proxy; this is a descriptive estimate from few cycles, not a verified2026 joint distribution. Total predictive variance is at least the full historical race-level variance. Adding duplicated polls cannot make a shared election shock disappear through division by source count.

Chronological diagnostics train only earlier election years:27 covered2020/2022 races have Brier0.06186. This is retrospective outcome accuracy with revised data and absent original release clocks/population labels. Its coverage differs from prior diagnostics, so scores are not directly comparable. No market prices, execution profits or original untouched forecast vintage are claimed.

For current Kansas Senate, the two verified-field primary polls give recency-weighted Democratic margin−1.126 points. Siena's published tie remains a separate sensitivity because detailed fieldwork is unavailable; it does not receive an invented recency date. The new model implies central Democratic settlement value0.398, with declared±3-point margin-shift scenarios0.226–0.605. These are transport estimates and assumptions, not calibrated confidence bounds. No research clock is renewed, existing bank rewritten, new paper wallet opened or competition order authorized by this calculation.

The useful change is horizon alignment and uncertainty conservation. Election settlement value cannot be substituted for a two-day price-reversion forecast. Source-weighting, portfolio risk and entry economics must retain uncertainty and compare against exact acquisition/exit depth. The old Kansas position remains attributed in its original journal.

Code: research/poll-horizon-v2.mjs. Three tests prove earlier-year isolation, duplicate-poll/common-shock preservation and unknown/future-field exclusions. data/poll-horizon-results-20261005-v2.json retains every diagnostic race, error component and source hash. No state-specific rule is embedded in the model.
