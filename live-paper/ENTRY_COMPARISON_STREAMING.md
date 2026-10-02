# Streaming recovery of the frozen comparison

The original host reads the whole root capture into one string. On the actual
capture after 19:55 UTC October2 it fails with `ERR_STRING_TOO_LONG`. Its failed
heartbeat makes the configured comparison risk projection unavailable, which
correctly prevents the basket overlay from replacing reads with unknown risk.

`entry-comparison-watch-v2.mjs` is a separate host recovery. The original fourteen
code-identity files, bank, cutoff, policies, model and original journal stay intact.
No new experiment can be initialized through this recovery host. It streams
bounded root records and original comparison checkpoints, verifies the entire
root chain through the unchanged pure reducer, verifies the original raw-byte
source prefix, and reproduces every saved checkpoint before appending progress.
It retains one current model state rather than the whole root capture in memory.

Live tail processing keeps the original bounded buffer, partial-record handling,
head/boundary/identity guards and durable journal accounting. Frozen bank bytes are
checked on every poll. STOP is checked again at post-replay and incremental commit
fences. Ordinary historical middle rewrites require full restart verification;
head/boundary checks do not prove the immutability of every old byte during append.

`Start-EntryComparison-v2.ps1` requires a terminal failed host, absent PID/lock,
existing bank/accounting and no operator STOP. It creates no client or API request.
Recomputed results after the outage are frozen-model counterfactual replay of
recorded future data, not orders made during the outage or realized competition
profit. Resumed stats identify the successor host and verified checkpoint count.

The tests compare exact original/successor wallet states, forbid a whole-source
read across large records, preserve partial tails, reject tampered checkpoints
and bank changes, and prove STOP prevents an advance after replay.
