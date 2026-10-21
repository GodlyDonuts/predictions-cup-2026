# Complete-day training reward

Training collection and sealed optimizer replay now share `training_reward.ledger`, and serial/parallel world construction share `training_reward.horizon`. An explicit inspected-development configuration can use:

```json
{
  "mode": "inspected_development",
  "development_training_daily_growth": true,
  "training_days": 2,
  "development_episode_ms": 172800000
}
```

Remove `development_episode_ms` or make it exactly the complete-day horizon. At least two integer consecutive 24-hour days must fit wholly inside the training partition. Short data is rejected, never clipped into a different reward. Reconstructed replay worlds must have exactly that horizon. Full-history mode continues to require complete days without the development flag.

The existing GrowthLedger computes the human mean daily log growth minus `1.5 * sample_daily_std / sqrt(D)`, applying `sqrt(N/(N+10))` only to positive growth. Dense transition rewards receive a terminal correction; sealed full-episode return targets include that correction. Development results remain ineligible for source or independent qualification. Whole financial state continues through each day; parallel worlds do not become additional independent historical days.

The frozen experiment records the selected reward mode and episode length. No API action, decision frequency or legal parameter domain is restricted by this change. Default short development runs retain their explicitly labeled log-wealth proxy.

The live population030 remains unchanged on its frozen one-hour configuration. Its pre-October17 training partition is shorter than 48 hours and therefore **cannot** use this complete-day mode. A later run requires a separately declared chronological development calendar with enough training history; changing the code does not silently change an existing experiment or make the requested full competition train/evolution/test split complete.

Verification: 23 training-reward, economics and population checks passed. A real synthetic batched episode records and reconstructs complete two-day ledger boundaries and terminal human rewards through EpisodeWriter/Reader. Its first-day gain followed by a flat day receives the negative consistency-adjusted score, rather than the positive short-window log-growth proxy. This is integration evidence, not market performance.
