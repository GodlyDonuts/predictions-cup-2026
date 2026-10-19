# Longer credit assignment without narrowing API choices

The first phase-011 learner used 805 updates. Across 5,848 active-world chunks,
the median virtual span was 425 ms; 70.14% ended before the modeled 600 ms order
arrival, and 77.41% before arrival plus receipt at 900 ms. Financial worlds and
recurrent state continued, and critic bootstrapping was used, so this does not
prove that short chunks caused the losses. It motivates a longer-context test.
[Diagnostic](training-chunk-credit-diagnostic-001.json).

## Measured compute changes

Encoder activation checkpointing recomputes intermediate activations during
backpropagation. Focused CPU tests match original outputs and parameter gradients
exactly. A real-data GPU benchmark ran eight worlds for 128 decisions each with
the 58,106,855-parameter actor, matching serial wallets, orders, fills and clocks.
Its accepted PPO update took 0.776 seconds and peaked at 2.801 GB allocated.
The collection comparison measured 11.864 versus 3.507 seconds for serial versus
batched processing under concurrent training load. This is a controlled kernel
benchmark, not whole-program throughput or ROI.
[Measured result](raw-long-context-benchmark-003.json).

The trainer previously hashed the entire model at collection and again before
each update. A real-checkpoint GPU measurement took a median 147.58 ms per hash,
versus 0.0827 ms for the optional process-local tensor-version identity.
The fast guard is limited to the fixed, buffer-free raw actor and supported
PyTorch version-counted updates, state loading and parameter replacement.
External raw-storage or `.data` alias writes bypassing version counters are
unsupported; the original full-hash mode remains available. The GPU benchmark
also verifies that the actual optimizer step changes the guard identity.
Persistent candidate/checkpoint hashes remain full byte/parameter hashes.
No trading reconciliation, dispatch, STOP or unknown-acceptance check is removed.
[Guard measurement](behavior-guard-overhead-001.json).

PPO now reports policy loss, value loss and entropy separately. Entropy strength
is an explicit learning setting stored with each optimizer, can vary across
founders, and evolves alongside learning rate. These learning auxiliaries never
enter external economic selection scores or change API/risk mechanics.

## Short-alarm initialization bug

The same latent clock head controls WAIT alarms and order expiration. The old
global short-alarm initialization biased both. In a declared synthetic context
using an actual fresh phase-011 founder, an expiring YES limit at .50 had an
82.43% probability of expiring before the modeled 600 ms arrival. Its median
lifetime was 16.43 ms. This is a conditional initialization diagnostic, not a
count of actual rejected orders or a population-wide estimate.

The corrected initializer changes only the existing trainable WAIT family
embedding, keeping ordinary order distributions at their neutral initialization.
Existing globally biased parents have an explicit linear reparameterization:
their WAIT distribution and order-quantity outputs are preserved up to floating
point while their expiration distribution changes. In the checked founder/context,
WAIT and quantity parameter differences were zero, and expiration-before-arrival
probability changed to 3.64%. No lifetime floor, fixed expiry, quantity limit or
new architecture is introduced. Optimizer moments reset after the migration and
new trajectories must be collected.
[Before](shared-clock-prior-diagnostic-001.json) /
[checked migration](wait-only-prior-migration-001.json).

## Next training profile

Phase 012 is prepared with eight preserved lineages/four islands, eight worlds,
128-decision chunks, encoder checkpointing, the process-local guard, wait-only
alarm initialization and entropy settings 0/.001/.01/.03. It uses the verified
observed-snapshot world and recorded information clocks on development data.
Completed and partial parent states must retain their explicit classifications;
this is not a new untouched test or evidence of profitable ROI. The complete
focused set passes 120 checks. Active-run evidence is recorded separately.
