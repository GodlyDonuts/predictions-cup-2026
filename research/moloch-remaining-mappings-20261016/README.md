# Three remaining mappings: candidate predictors, not equivalent payouts

The October 16 audit adds **zero equivalent mappings**. Markets 191, 276 and
280 can retain their named-candidate references as **unqualified Predictive**
inputs. They cannot become direct payout matches merely to raise the existing
234-direct count. No feed, configuration, account or production changes were made.

|SIG market, historical catalog label|Retained Kalshi candidate contract|Concrete finding|
|---|---|---|
|191, Independent / Rhode Island Governor|`GOVPARTYRI-26-KBLO`, Ken Block|Official candidate reader lists Block, Gotra and Reynolds as Independent and on the election ballot. Block alone does not exhaust even the currently listed Independent candidates.|
|276, Independent / Montana Senate|`SENATEMT-26-IND`, Seth Bodnar|Original official general-election HTML lists Bodnar as IND, and a separate NON write-in. Neither that write-in nor every possible successor is silently classified as Independent.|
|280, Independent / Nebraska Senate|`SENATENE-26-DOSB`, Dan Osborn|The original certified general-election list identifies Osborn as **By Petition**. That ballot category does not define SIG's missing Independent-party predicate.|

These SIG descriptions are historical catalog identities, **not recovered
contract rules**. The execution owner reported a finite scan of 51,164 retained
official receipts with no nodes/detail receipt for these three markets. This
child made no SIG account requests. Contract IDs, scoped roots, precise stage,
party-affiliation resolver and target-specific settlement clocks remain unknown.
Rules from a different SIG market cannot fill those fields.

## What the original sources establish

[resolution-evidence.json](resolution-evidence.json) pins the existing universe
SHA256, historical catalog, three original Kalshi captures, exact JSON pointers,
compressed and decoded receipt hashes, and original body hashes. All three
`bodyText` hashes and parsed-body equality were independently rechecked. Those
captures were received at **09:28:39–41 UTC**, not refreshed prices at audit time.

The Kalshi primary predicates require the named person to be inaugurated
(Rhode Island) or sworn in for the term beginning in 2027 (Montana/Nebraska).
Their secondary excerpts permit accelerated determination and refer to fuller
rules. These excerpts do not supply a complete tie, replacement, cancellation
or refund state table. The displayed `mutually_exclusive` flag does not certify
exhaustiveness or prove `Independent = 1 − Democratic − Republican`.

Primary candidate checks:

- [Rhode Island official candidate reader](https://vote.sos.ri.gov/Candidates/CandidateSearchSummary?Election=18144&OfficeType=549): inspected during this audit; its displayed update is October 16 at 7:26 AM, without an explicit timezone. The original direct GET returned **403**. The separately hashed review extraction is explicitly **not** an original HTTP-body hash; the blocked response is not ballot proof.
- [Montana official 2026 general-election list](https://candidatefiling.mt.gov/candidatefiling/CandidateList.aspx?e=450002987): original HTML received **17:11:07 UTC**, SHA256 `64aaa451ed036eb6f3cac97db6493592e2a4e0291ee12be650fb9bfbeb3ac7c2`. This corroborates the named candidate and office, not a SIG payout resolver.
- [Nebraska certified general-election list](https://sos.nebraska.gov/sites/default/files/doc/elections/2026/Final_Statewide_General_Candidate_Filing_List_9.11.26.pdf): original PDF received **17:11:36 UTC**, SHA256 `d27a071715896b8c82b8cce8d48200b4207ba10c44f1778f56f92c4efcd7cf96`; page 1, November 3 general election, document dated September 11. The parsed first page independently contains Osborn. Candidate contact information remains outside Git.

Usable original public HTML/PDF and the blocked-response record are retained
privately under `Temp/moloch-remaining-mappings-20261016`. No authenticated
request, Kalshi API request, secret or private account payload was used.

## Exact remaining basis risk and useful next step

A different Independent winning, a named winner failing to take office, a
replacement nominee, or changing affiliation can separate a generic party
predicate from a named-person predicate. These are counterexamples to an
*assumed* generic mapping, not assertions about SIG rules that are unavailable.
Fusion, runoff and tie handling also remain unresolved. The retained official
[SIG settlement guide](https://sig.thesuper.market/docs/settlement-and-payouts)
documents principal refunds for cancellation/N/A; that is not proof of matching
Kalshi exceptional cashflows.

Keep candidate prices/flow in a distinct Predictive channel, with original
availability clocks and basis-risk labels. Cross-race or chamber-control
references are Factor inputs only; no coefficient, hedge ratio or predictive
performance was established here. In particular, a named candidate's NO price
is not the complement of *any* Independent winning. Any later model needs its
own prospective validation; this audit supplies neither pricing authority nor
profit evidence.

Upgrade only after original target-specific SIG rules and complete Kalshi
exception/finality evidence support a reviewed cashflow-state comparison.
Until then, preserve all three conditional analog classifications and leave
the live mapping unchanged.
