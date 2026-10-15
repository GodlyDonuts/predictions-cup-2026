# Preserving a competition-scale wallet across source episodes

`run.mjs --format journal --follow --resume PREVIOUS_DIRECTORY` restores a completed paper tail without new capital or policy changes. It requires the parent's terminal report, checks its snapshot hash, reproduces the consumed source-prefix SHA256, verifies every wallet journal link and replays every state hash. Continuation parents are checked recursively, with cycle/depth rejection. Running or failed tails lacking a terminal report cannot be resumed through this path.

For the same source, already consumed rows are verified and skipped. Unseen receipts received during the restart interval are processed with their original receipt/execution clocks and a new actual processing time. For a new source journal, its chain is verified from row one. A new paper bank still skips pre-start receipts; carried banks retain delayed-fill exposure. Old book source clocks prevent replayed receipts from fabricating fresh entries. The manifest/snapshot records the exact parent hash and zero new paper capital. `--policy` is rejected on resume.

`live.mjs --rest-recovery ... --recovery-tail PREVIOUS_DIRECTORY` can include a verified independent wallet's expired quote windows in the one existing collector. Source ranges are merged; no independent cash, holdings, intents or orders are imported into the collector's financial wallet. Every unresolved order and owned inventory scope must remain captured. Future unexpired quote windows use ongoing normal tape captures; their funding is never released merely because a process stopped.

This preserves the existing account mutex, read pacing, backoff and zero contest writes. A smaller source scope is permitted only after actual process termination and liability checks. A terminal report file is verification data, not a process lock: the operator also checks the live process handle before changing runtime ownership. Three dedicated tests cover exact restoration and intervening receipts, parent/source tampering, missing terminal reports and omitted recovery liabilities. Full Moloch suite: 106 passing tests.

## Verified runtime application

Source 31 and scale tail 1 exited zero after graceful STOP. The scale parent replay reproduced its 100,000 cash, zero fills and exact reserved liabilities. Empty scopes 884/1038 were retired after financial checks; inventory scope 970 and entry scope 841 remain. Source 32 includes the scale wallet's expired 841 recovery window, and scale tail 2 resumes the exact parent hash with zero new capital. Both new process handles were checked live.

Observed consecutive 841 book intervals changed from 101.587/99.548/99.330/103.496 seconds to 49.671/49.731 seconds, below the existing 60-second source-silence cutoff. No read budget was raised and no actual order was submitted. This improves continuity but does not establish profitable execution. Timestamped checked evidence: `data/moloch-source-continuation-20261014.json`.
