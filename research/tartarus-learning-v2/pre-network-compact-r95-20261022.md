# R95: bounded recovery metadata, not a production restart

R94 remains the installed production worker, source6b7d96c8/binary77bad67cb78a. Its quote21832 original no-send outcome contains6,531,760 bytes. Fundamental dispatch scanning retains an8MiB per-row limit and64MiB whole-journal bound. Repeating full canonical queue states inside every recovered-expiry outcome could consume those bounds and interrupt execution.

R95 retains the exact original SDK manifest, guard, checkpoint and journal in the existing private source capsule. Newly captured dispatch proof metadata contains the original-context hash, a hash of the complete opaque SDK evidence, the original journal hash and capsule path. Startup still runs the same canonical SDK verifier against original bytes before restoring process-local trust. The proof is not inferred from a hash or a serialized boolean. Focused fixtures require metadata below4KiB.

Previously committed full SDK proof rows are reverified byte-for-byte and remain unchanged. In addition to270 worker and149 fundamental checks, three focused checks cover compact append/restart, corrupted/changed source rejection, real network-marker exclusion and legacy full-proof restoration. The new verifier also read the actual original21832 retained dispatch/proof successfully without constructing an account transport or changing that log.

This is an offline-qualified continuation. It changes no economic model, monetary limits, native engine, accounting attribution or live order. No R95 deployment or achieved cadence/ROI is claimed. Keep R94 running while observing its actual execution; this source change alone is not a reason to interrupt a healthy worker.

[422-check qualification](r95-pre-network-compact-qualified-20261022.json), [actual original legacy-proof verification](r95-original-legacy-proof-20261022.json). Qualified offline binary `bda95858eca810667c6f2362df19678c25461cc8a3ebf57f0623adbbdf02c10d` is not installed.
