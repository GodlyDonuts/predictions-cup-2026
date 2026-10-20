# Short screens do not establish longer-horizon trading quality

GPU019 continues on its frozen source and GPU020's full-return comparison remains queued. A new CPU-only diagnostic checks already inspected selection price paths; it does not alter either run or open held-out data.

For each exchange, the diagnostic tracks the lowest earlier displayed YES ask and a later displayed YES bid. It records a positive one-share difference when the later bid exceeds that earlier ask. Books must pass the replay's 60-second freshness check at observation. Original source hashes and entry/exit times are retained. No paper orders, fills or actual API calls occur.

| Selection interval | Duration | Exchanges with positive displayed path | Largest one-share quote difference |
|---|---:|---:|---:|
| First short screen | 120s | 1 | 0.005 SUSQie |
| Second short screen | 120s | 1 | 0.035 SUSQie |
| First complete selection stratum | 1,499s | 9 | 0.010 SUSQie |
| Second complete selection stratum | 1,499s | 12 | 0.050 SUSQie |

These are **hindsight quote-path differences**, not earned or executable profits. The diagnostic assumes perfect future knowledge and free simultaneous full-universe quote visibility. It ignores observation/dispatch latency, queue position, traded depth beyond one share, fees and exact account/market eligibility. A displayed quote can disappear before admission. It does not bound passive market-making opportunities, mark-to-market outcomes, NO-side paths or portfolio performance. Distinct-market counts also are not independent profitable event counts for the human's reward function. [Exact data, clocks, source hashes and assumptions](displayed-path-hindsight-audit-002.json).

The counts show a limitation of short screening: few aggressive round trips are visible within each short interval, and longer intervals reveal additional price paths. They do not prove that a learned policy can predict or capture any of those paths. The zero-fill candidate's flat screen remains a no-edge result. Short negative or flat scores alone cannot establish that a policy lacks useful longer-horizon behavior, particularly when its observation includes time remaining to the horizon.

The existing full-window confirmation of positive screen winners remains connected, but that positive-only trigger does not examine every possible longer-horizon candidate. Broader longer-horizon evaluation/selection evidence remains required before strategy or profitability qualification. Full-episode training on a short horizon removes estimated return bootstraps; it does not manufacture long-horizon evidence. This remains an open limitation of the development program, alongside the missing requested multi-day history and untouched test coverage.

No classifier was trained on these hindsight paths, no path was fed to an actor, and no production, STOP, activation, risk or financial state changed. The live learner and queued learning-target comparison were left intact.
