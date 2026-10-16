# Buffered report writer: actual ARM64 result

Source `31aefba5c22b529d60acab65659876294f2e0c8d` changes native atomic JSON
serialization to a bounded 64KiB buffer. Explicit flush, file sync, atomic
rename, directory sync, exact JSON bytes and final newline are retained.
The only other input added is an offline benchmark binary. All other 243
predecessor inputs are identical, except the precisely reviewed Session file.

The actual candidate built on the production VPS separately from its running
worker. Its runtime SHA256 is
`0fa66973f3f51144b1e56dadd0f1453b9ac6c49f9d0279a61a53aee8a99e22f4`;
engine identity is
`8acc461bc02fb9236100058fe464b203d5da005375632af05c176223c7246754`.
These are candidate identities, not a claim of production promotion.

At 12:28:59 UTC the original frozen 1,878,778-byte production report was
serialized by both implementations into new temporary files:

| Measurement | Unbuffered | Buffered |
|---|---:|---:|
| Physical write calls | 668,119 | 29 |
| Wall time | 682.431191ms | 13.663443ms |
| Exact original byte/hash parity | Yes | Yes |
| File and directory sync | Yes | Yes |

This single pair measured 49.95 times faster report writing and 23,038.6 times
fewer write calls. It is **not a measured whole-loop or trading speedup**.
Sequential cache effects and nice19 CPU scheduling remain limitations. There
were zero network requests, no financial replay, no original-source mutation
and no production change in the benchmark.
[Original aggregate output](offline-write-benchmark.json), raw SHA256
`8007c404151eb51f9c8042a0ba7ab6fa6868a1ee165dc4f29b407090a40f28b7`.

The actual locked, offline ARM64 build passed 72 native-library tests,
147 runtime-library tests, 78 worker tests and four detached-copy helper tests:
301 distinct passes, zero failures. Three buffer byte/failure tests are already
included in the 72, not added again. One native test was ignored and is not
credited. Native helper and runtime release builds passed. Root independently
verified all 244 source hashes, artifact hashes, original and new engine
identity formulae, and actual test-log hashes.
[Target-specific validation](operator-validation.json), raw SHA256
`6903b020293a9b0994dc742472bced2304da76b955ee720bde7be20d92ca9e1d`.

The reviewed operator stages immutable binaries and changes only the activation
binary pin in the existing control. A cooperative original-worker stop,
unchanged first eleven bridges, genuine twelfth bridge and complete original
stopped-prefix replay on a detached copy precede promotion. The original bank,
funding, daily windows, order liabilities and restart conditions remain.
Actual deployment and subsequent loop measurements must be recorded separately.
