# Faster verified paper startup

The paired runner previously replayed the same terminal bank three times: once to check the registered policies and once for each consumer. It now performs one full reconstruction, then each consumer rechecks an in-memory proof against exact bytes before cloning the bank.

Only a result produced by the current process's full `completedTail` verifier can be reused. Every ancestor snapshot, report and event journal is hashed; every consumed source prefix is hashed to its original bound. Snapshot-object mutation, forged/copied tokens, changed current or ancestor evidence, missing files and short prefixes fail closed. An unrelated appended source suffix remains outside the previously consumed financial history. There is no persistent cache, assumed process lock, wealth reset, or bypass for a mutable proof.

On the real preserved parent, full reconstruction measured18,319.98ms and exact-byte rechecking37.99ms. This removes repeated financial replay from future paired startup; it does not eliminate the first verification or account-source initialization. Evidence: `data/moloch-parent-proof-timing-20261014.json`. All153 core tests pass, including tampered current/ancestor evidence and fabricated-proof rejection. The active phase63 loaded its original code and is not changed retroactively.
