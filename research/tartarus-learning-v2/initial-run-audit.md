# Initial training run audit

The first checkpoint and full result remain unchanged. Its development losses are not discarded.

Subsequent implementation review found two simulator/runner issues: a source gap discarded the local depth object and therefore its counterfactual depletion; and the initial60-second source burn-in populated the environment but did not replay the actor recurrence. The follow-up preserves depleted liquidity through invalidation/recovery and includes60 explicitly non-learning recurrent warm-up frames in each trajectory. These limitations qualify the first historical result; its published numbers must not be presented as final environment evidence.

The policy likelihood now excludes unused trade heads for WAIT/CANCEL and excludes the unused target fraction for EXIT. PPO also stops further epochs when sampled policy divergence exceeds the declared threshold. These are learning corrections, not changes to exchange time, fees or production limits.

An independently compiled offline Rust wallet oracle matches ten exact Python states: funding reservations, partial execution prices/fees, FIFO disposal, cancellation release and complementary YES/NO netting. This is wallet-semantic parity, not full transport/environment differential certification or execution calibration.

The new population code implements four logical islands, sequential GPU residency, compatible checkpoints, optimizer reset after mutation, fresh-trajectory enforcement, ancestry and protected development alternatives. Mutation size is controlled by measured market/family categorical KL on training-only recurrent probes, with raw normalized output distance also reported. Full joint-action KL and projected execution distance remain unavailable and are explicitly null. Repeated development evaluation is not locked testing.

Further review found that the original second development search block had no fresh SIG books. The full population's zero returns in that block cannot establish robustness. A revised split is source-coverage-driven and all learners restart under a new role manifest.

Short-patience drafts previously could pass a long preflight and still be sent after their local decision deadline. They now retire without a write when patience expires before admission; genuinely in-flight orders retain cancel/fill races. Initial result artifacts are preserved rather than rewritten. PPO now backtracks the actual first step as well as stopping later excessive-divergence epochs.
