# Actual ARM64 build qualification

Candidate source 43b0341a15a3ef0d072c34724a3ef7a169d7594e, binary d8e4672a8b5a90f406de830846a9388aaa3922a4a34cabfe3077165ef6cced72, unchanged native identity 5b61d3cbb38eadf5170fafae5c91cb5b75858346b2771031084224bfa2c3d6d4.

The actual ARM64 worker command passed 134 cases; the lifecycle command passed 14. The earlier 21-case library result is reused only after its exact source inputs were verified unchanged. The locked, offline optimized release command completed successfully. These are command-level counts, not a profit estimate.

Root independently checked all 287 original private packet files, the actual binary, all 220 Rust/TOML/lock inputs and all 263 packaged repository files against this Git commit: 59 exact, 204 differing only by CRLF. All sixteen exact native source inputs reproduce the unchanged native identity. The private archive is 7,323,123 bytes, SHA-256 f57dce3db2434782b56159c5bba193a9269b407c655c794ed5c337a917939a81.

[Original qualification](qualification-final.json), [root verification](root-verification.json), [all source pins](actual-source-manifest.json) and original command logs are published here. Earlier fixture failures and original source versions remain in the private packet; no production process was stopped for those attempts.

This qualifies the built implementation. Installation, restored operation and measured economic benefit require separate evidence.
