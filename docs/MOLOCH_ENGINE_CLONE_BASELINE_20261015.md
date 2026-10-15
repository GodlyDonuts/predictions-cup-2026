# Native Engine clone baseline before changing book ownership

Task: [#133](https://github.com/GodlyDonuts/predictions-cup-2026/issues/133). This offline harness uses synthetic in-memory market events. It opens no account client, production bank, source journal or credential. It does not change the financial engine, session, activation or runtime. Production remains under [#120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120).

## Hypothesis and source evidence

`Engine::advance` clones the entire engine before applying and auditing an event, then commits the candidate. Online `Session::accept` first clones the engine itself, so it adds another full clone before `advance`. Startup journal replay calls `advance` once per event. This transaction pattern preserves rollback and must remain intact.

Books currently own their complete raw JSON plus parsed bid/ask vectors. `BTreeMap<String, Book>` therefore deeply clones every retained book in each engine clone. The desired-target pass also copies `Book` into each `Target` at five construction sites. With tournament-wide observation this can repeatedly copy237 depth20 books. These are source-level facts; their measured contribution to production delay remains to be established.

The proposed later optimization is `BTreeMap<String, Arc<Book>>` and `Target.book: Arc<Book>`. New book events would insert a newly parsed `Arc<Book>`. The only retained-book mutation identified in the engine is gap invalidation, which must use `Arc::make_mut` before setting `invalid`. Order construction must explicitly clone the three identity strings instead of moving them out of an Arc. Other engine maps/vectors, JSON reporting and per-event filesystem synchronization still cost time; this optimization cannot be assumed to solve all callback lag.

## Deterministic baseline

The fixture warms237 synthetic markets with20 levels per side, exact tick prices and the ordinary live engine mode. It enables the existing native draft-order logic but submits no physical orders or fills. Three pacing-respecting ticks populate additional orders while every book remains within the normal ten-second freshness bound. No native rate, policy validation or paper-fill rule is bypassed.

The binary measures clone-and-drop wall time separately from `Engine::advance` for tick, full book replacement, and alternating gap/resync events. All workloads begin from the same warmed state. Five prefixes include the initial state and quarter checkpoints. Their financial-state SHA-256 covers report, order records, fills, known/required exchanges and actual-order bindings. It is not a hash of private internal allocation layout or a proof that every hidden field is serialized.

A rejected duplicate-member gap deliberately mutates the first candidate book before rejecting the second member. Its public financial fingerprint must remain unchanged. A separate regression then compares the next valid event with a freshly and independently constructed engine, which helps detect invalidation leaking through shared book ownership.

```sh
cargo test --locked --manifest-path moloch/native/Cargo.toml --test engine_clone_benchmark
cargo build --release --locked --manifest-path moloch/native/Cargo.toml --bin moloch-engine-clone-bench
nice -n 10 /usr/bin/time -v moloch/native/target/release/moloch-engine-clone-bench --iterations 100
```

Run at reduced scheduling priority after other validation workloads finish. Record the binary SHA, engine identity, source commit, architecture, iterations, wall times, peak RSS and complete JSON output. Run the identical fixture/settings against any candidate and require all prefix hashes and rollback results to match before interpreting timing. Fixture generation and fingerprint hashing are excluded from the reported advance timing; warm setup time is reported separately.

The harness measures in-memory work only. It does not measure session fsync, complete private startup replay, live callback throughput, fills or profit. Native source identity must remain unchanged for this baseline: the new binary/support/test/docs files are outside the existing engine identity input list. Any later Arc engine change requires full correctness/parity checks and an explicit source-prefix upgrade proof before the existing production bank can open it. No migration is authorized by this benchmark document.

## Evidence status

The harness and four regressions are source candidates. Local Windows has no Rust compiler; ARM64 Linux results are pending. No speedup, dominant bottleneck or production behavior change is claimed until those measurements exist.

## Verified ARM64 candidate results

The Arc candidate passed 94 native and 115 runtime checks, including clone rollback and immutable prefix validation. The identical 237-market, 20-level, 100-iteration synthetic fixture measured mean clone time 4.927630 to 0.042951 ms and tick advance 17.820968 to 0.248175 ms. All 15 financial prefix fingerprints and rollback results matched. These timings exclude network and filesystem synchronization.

An actual frozen production prefix of 16,535 events / 45,816,161 bytes replayed in 48,163 ms before and 14,125 ms after. Both verified the exact original hash chain and identical financial report, orders, fills, source cursors and actual-order bindings. Neither checker modified the source, authorized migration or contacted the account. The three checker regressions cover valid immutable input, partial-tail refusal and financial-report mismatch refusal.

Sanitized evidence is in research/moloch-engine-book-cow-20261015: engine-clone-baseline.json, engine-clone-arc.json, engine-clone-baseline-resources.json, live-prefix-baseline.json and live-prefix-arc.json. Python subprocess/resource supplied resource measurements because /usr/bin/time was unavailable. This supersedes the pending-results paragraph above. Production still requires an exact-prefix identity bridge after the prior worker has actually stopped. Replay speed is separate from live latency, successful exits and trading profit.
