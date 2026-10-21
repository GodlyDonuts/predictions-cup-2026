# Shared immutable native review history — October21

The original2.49GB replay benchmark exposed a large state-copy cost:64 events took roughly16.6seconds with repeated transactional Engine copies, compared with0.234seconds using the consuming unpublished-state path. That comparison includes all Engine copies; it does not attribute all the difference to one field or predict end-to-end live speed.

Three historical review-proof maps now use reference-counted copy-on-write storage. Reading or cloning an Engine shares identical immutable maps. Inserting or modifying a review first makes that map private. All original proof values, report rendering and order-record rendering remain, and failed staging cannot mutate its original Engine. No financial equations, lifecycle checks, orders, rate limits or strategy selection changed.

126 ARM64 checks pass, including genuine financial/restart coverage and a focused large-proof test that verifies sharing on clone, isolated writes in every map and unchanged original reports. The full copied original743,614-event financial-parity replay and the same64-event timing comparison are running offline against the previously pinned source. Those results are pending; this change is not deployed and no live speed gain or profit is claimed. Production R61/996893 continues restoring the prior checked native source.
