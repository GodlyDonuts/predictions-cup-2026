# Capital allocation correction — October 23, 2026 UTC

The human requested winning the competition and rejected activity without
results. Winning, restored profitability and a qualified strategy are still
unproved. This change is offline research preparation, not a production fix.

The latest retained official account audit at 01:03 UTC reports 93,509.52
SUSQies, -6.49048% versus the official 100,000 initial balance. The later
01:49 original-receipt audit reports only 33.63 cash, 94,211.735 remaining lot
cost and 757 fill rows. Cost is not equity. The most recent purchase earned
zero realized profit. These are distinct dated observations, not live state.

The current `run-current-money-review-once.py` calls only the standalone
opportunity scanner. A positive standalone BUY must fit cash before any sale;
a positive combined sale/replacement can therefore be excluded even when its
first sale releases the required actual funding. The repository already has
separately audited rotation screening and preparation, but the current
one-shot controller does not compare them. This gap matters with 33.63 cash.
It does not establish that a profitable executable rotation exists today.

The new `tools/prepare-capital-aware-money-research.py` runs both existing
scanners against the same captured portfolio and original reference inputs.
It compares each independently audited candidate using the lower of its
sampled-model and ordinary-interval expected improvements. Every proposed
value BUY, including a conditional later replacement, needs matching managed
inventory coverage. Equal scores favor a direct trade. The selector preserves
the exact chosen direct quantity when different tranches share a market.
The original preparers recompute funding, FIFO, shared depth, clocks and
nomination evidence. Default price tolerance is zero.

The output is an **unapproved** nomination. A rotation's positive score belongs
to its entire conditional sequence; the first sale may surrender expected
value and a later purchase may never become executable. No proceeds are
credited in advance. Publication still requires the existing authenticated
owner, exact current-binary conditional-rotation qualification, original
review completion and generation checks. A later purchase requires an actual
sale fill, a fresh account/book capture and a separate current review. This
tool cannot publish, trade or install itself into the existing controller.

Example on the production host, using fresh inputs captured through the
existing worker and its existing public-data relay:

```text
python3 prepare-capital-aware-money-research.py --source SOURCE_ROOT --tools CHECKED_TOOLS_ROOT --inputs FRESH_MATCHED_INPUTS --output NEW_PRIVATE_OUTPUT
```

Qualification: 11 selection/orchestration tests and 5 independent existing
funding tests pass on Windows Python 3.12. CLI loading, Python compilation and
Git whitespace checks pass. Orchestration tests use mocked scanners/preparers;
no complete original production snapshot is available here for an end-to-end
replay. Linux production execution is unverified. No worker was restarted and
no live account request, order, source-manager change or new profit occurred.

Access blocker: the recorded `archon@100.80.176.110` rejects BatchMode SSH with
`Permission denied (publickey,password)`. Existing local SSH processes are
present; they do not grant this process an authenticated reusable connection.
The human has been asked for the existing approved SSH profile/key path.
No credential was created, copied or read. The separately claimed source022
handoff is untouched.

Public verification during this review: the official leaderboard lists first
place at approximately +985.4k, with 4,834 trades; this is provisional marked
profit, not opposing cash or final wealth. The official rules and settlement
guide confirm final ranking follows settlement of all markets, after trading
closes November 4 at noon Eastern. A catch-up strategy must therefore address
terminal wealth and profitable capital reuse, not order count alone. Rival
holdings and actual winning probabilities remain unknown.

Sources: [latest retained account](official-marked-account-20261023.json),
[latest retained receipts](r105-first-filled-audit-20261023.json),
[official leaderboard](https://sig.thesuper.market/leaderboards),
[official settlement guide](https://sig.thesuper.market/docs/settlement-and-payouts),
[official rules](https://predictionscup.com/rules/).
