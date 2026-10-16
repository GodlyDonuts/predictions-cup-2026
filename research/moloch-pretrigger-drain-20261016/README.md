# Prospective trade-trigger ordering — actual V7 rollout

Read-only observer545515 started21:05:23UTC on the actual545165 SIG breadth
journal. Its exact producer is3d3b5d43a6c79293; the mapping remains3f8dd6fa.
It runs for one hour, with a2GiB combined compressed cap and10GiB free-space
floor. No SIG restart or additional account client was used.
[Actual start and pins](v7-rollout.json).

After pinning and parsing a complete original external response, the observer
drains currently readable original SIG rows before the trade callback, then
takes its decision clock. Original source/cache/trade clocks and all2s/3s/20s
limits remain unchanged. Each drain is bounded to4MiB and at most one actual
EOF rotation; it never closes the tail generator mid-row. A source-bound
sigDrainPolicy version2 permits only this ordering in the fixed verifier.
The existing container protocol and sampling-policy hash remain unchanged.
Old skipped trades are not relabeled or rescued.

The root executed106 checks on Windows and106 on actual Linux:42 observer,
17 trade-sampling,16 input-trace and31 fixed-replay checks. The initial Linux
packages omitted required Python/protocol dependencies; those packaging failures
were retained. The final pinned package passes all four suites.
[Actual Linux validation](validation-r4.json), [source pins](source-pins-complete-r4.json).

V6 normally sealed at20:30:59UTC with9 rows/3 observed markets and no pending
samples; its complete original trace remains on the VPS. V7's later initial
status at21:10:01UTC records7 samples,5 observed markets and14 labels, with2
pending samples. This does not establish improved economic yield, a qualified
model or trading profit. No model was promoted.
[Original V6 seal](v6-input-trace-seal.json), [V7 observation](v7-initial-status.json).

[artifact-pins.json](artifact-pins.json) preserves exact evidence bytes. The
start script is the record of an already executed operation; do not rerun it.
