#!/usr/bin/env python3
"""Owner-reviewed, offline-only one-pair Session benchmark. Never reads a live bank."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import time

FILES = ("manifest.json", "head.json", "report.json", "events.jsonl", "engine-upgrade-v1.json")
INPUTS = tuple("src/" + x for x in ("engine.rs", "engine_live.rs", "reviewed_inventory_exit.rs", "reviewed_fundamental_entry.rs", "ordinary_entry_exclusions.rs", "live_market_capacity.rs", "engine_capital.rs", "semantic_floor.rs", "engine_packages.rs", "probabilistic_packages.rs", "engine_collateral.rs", "lib.rs", "legacy.rs", "journal.rs", "session.rs")) + ("Cargo.lock",)
DIGEST = re.compile(r"[0-9a-f]{64}")
MAX_FILE = 2 * 1024**3

def require(value, code):
    if not value:
        raise ValueError(code)

def sha(value):
    return hashlib.sha256(value).hexdigest()

def signature(s):
    return (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns, s.st_nlink)

def pin(path, expected=None):
    path = Path(path)
    require(path.is_absolute() and path.resolve() == path, "PATH_CANONICAL")
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1 and before.st_size <= MAX_FILE, "REGULAR_SINGLE_LINK_BOUND")
    h = hashlib.sha256()
    with path.open("rb") as stream:
        require(signature(os.fstat(stream.fileno())) == signature(before), "OPEN_IDENTITY")
        while block := stream.read(1024 * 1024):
            h.update(block)
        require(signature(os.fstat(stream.fileno())) == signature(before), "READ_IDENTITY")
    require(signature(path.lstat()) == signature(before), "PATH_CHANGED")
    result = {"path": str(path), "bytes": before.st_size, "sha256": h.hexdigest(), "device": before.st_dev, "inode": before.st_ino}
    if expected is not None:
        require(result["bytes"] == expected["bytes"] and result["sha256"] == expected["sha256"], "PIN_MISMATCH")
    return result

def write_new(path, value):
    with Path(path).open("x", encoding="utf-8", newline="\n") as stream:
        stream.write(json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n")
        stream.flush()
        os.fsync(stream.fileno())

def private_root(value, prefix):
    path = Path(value)
    require(path.is_absolute() and path.resolve() == path and str(path).startswith(prefix) and path.name not in (".", ".."), "OFFLINE_ROOT")
    require("live-production-v1" not in str(path), "LIVE_ROOT_FORBIDDEN")
    return path

def source_identity(root, expected):
    root = Path(root)
    require(set(expected) == set(INPUTS), "SOURCE_INPUT_SET")
    h = hashlib.sha256()
    checked = {}
    for name in INPUTS:
        checked[name] = pin(root / name, expected[name])
        h.update((root / name).read_bytes())
        h.update(b"\0")
    return h.hexdigest(), checked

def proposal_check(proposal, original, head, report, prefix, old_id, new_id):
    require(len(original["upgrades"]) == 12 and len(proposal["upgrades"]) == 13, "EXACT_TWELVE_PLUS_ONE")
    require(proposal["upgrades"][:12] == original["upgrades"], "ORIGINAL_BRIDGES_CHANGED")
    last = proposal["upgrades"][-1]
    require(last["fromEngineIdentity"] == old_id and last["toEngineIdentity"] == new_id and old_id != new_id, "BRIDGE_IDENTITY")
    require(last["oldHead"] == head and last["oldReportEnvelope"] == report, "BRIDGE_ORIGINAL_PARITY")
    require(last["journalPrefix"] == {"bytes": prefix["bytes"], "sha256": prefix["sha256"]}, "BRIDGE_PREFIX")
    require(last["previousUpgradeHash"] == original["upgrades"][-1]["hash"], "BRIDGE_CHAIN")

def invoke(helper, args, output, name, seconds, json_output=True):
    import resource
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    started = time.monotonic_ns()
    with (output / (name + ".stdout")).open("xb") as stdout, (output / (name + ".stderr")).open("xb") as stderr:
        try:
            result = subprocess.run(["/usr/bin/nice", "-n", "19", str(helper), *args], stdout=stdout, stderr=stderr, timeout=seconds, check=False)
            code = result.returncode
        except subprocess.TimeoutExpired:
            code = "timeout"
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    receipt = {"exit": code, "wallNs": time.monotonic_ns() - started, "userCpuSeconds": after.ru_utime - before.ru_utime, "systemCpuSeconds": after.ru_stime - before.ru_stime, "childMaximumRssKiBCumulative": after.ru_maxrss, "stdout": pin(output / (name + ".stdout")), "stderr": pin(output / (name + ".stderr"))}
    write_new(output / (name + ".execution.json"), receipt)
    require(code == 0, "PROBE_FAILED_" + name)
    raw = (output / (name + ".stdout")).read_bytes()
    require(raw.endswith(b"\n") and raw.count(b"\n") == 1 and len(raw) < 64 * 1024**2, "PROBE_STDOUT_SHAPE")
    return (json.loads(raw) if json_output else raw.decode().strip()), receipt

def copy_bank(source, output, name, pins):
    target = output / name
    target.mkdir(mode=0o700)
    for filename in FILES:
        with (source / filename).open("rb") as incoming, (target / filename).open("xb") as outgoing:
            shutil.copyfileobj(incoming, outgoing, 1024 * 1024)
            outgoing.flush()
            os.fsync(outgoing.fileno())
        pin(target / filename, pins[filename])
    write_new(target / "OFFLINE_ONLY.json", {"offlineOnly": True, "productionAuthorization": False, "productionStopped": False})
    return target

def unchanged(bank, pins, candidate=False):
    for name in FILES:
        if not candidate or name != "engine-upgrade-v1.json":
            pin(bank / name, pins[name])

def run(manifest_path, expected_manifest):
    require(sys.platform.startswith("linux"), "LINUX_OFFLINE_ONLY")
    require(DIGEST.fullmatch(expected_manifest), "MANIFEST_SHA")
    manifest_pin = pin(manifest_path)
    require(manifest_pin["sha256"] == expected_manifest, "MANIFEST_SHA")
    m = json.loads(Path(manifest_path).read_bytes())
    require(m["schemaVersion"] == 1 and m["approvedOfflineOnly"] is True and m["productionAuthorization"] is False, "OFFLINE_REVIEW_REQUIRED")
    source = private_root(m["frozenRoot"], "/tmp/moloch-native-flow-frozen-")
    output = private_root(m["outputRoot"], "/tmp/moloch-native-flow-benchmark-")
    require(source != output and source not in output.parents and output not in source.parents, "DISTINCT_ROOTS")
    require(set(m["files"]) == set(FILES) and 1 <= m["maximumProbeSeconds"] <= 3600 and m["expectedBridgeCount"] == 12 and DIGEST.fullmatch(m["oldBinarySha256"]), "MANIFEST_BOUNDS")
    output.mkdir(mode=0o700)
    require(shutil.disk_usage(output).free >= 8 * 1024**3, "DISK_HEADROOM")
    # All original and executable/source pins are frozen BEFORE report/bridge
    # parsing, proposal construction or any Session replay/outcome access.
    pins = {name: pin(source / name, m["files"][name]) for name in FILES}
    builds = {}
    for role in ("baseline", "candidate"):
        b = m[role]
        identity, checked = source_identity(b["nativeSourceRoot"], b["sourceFiles"])
        require(identity == b["engineIdentity"], "RAW_SOURCE_IDENTITY")
        builds[role] = {"engineIdentity": identity, "sources": checked, "probe": pin(b["probePath"], b["probe"]), "buildEvidence": pin(b["buildEvidencePath"], b["buildEvidence"])}
    require(m["oldBinarySha256"] == builds["baseline"]["probe"]["sha256"], "DETACHED_BASELINE_BINARY_BINDING")
    differences = [name for name in INPUTS if builds["baseline"]["sources"][name]["sha256"] != builds["candidate"]["sources"][name]["sha256"]]
    require(differences == ["src/engine.rs"], "ONLY_FROZEN_ENGINE_DELTA")
    require(m["baseline"]["optimizedProfile"] == m["candidate"]["optimizedProfile"] and m["baseline"]["optimizedProfile"] == "release", "MATCHED_RELEASE_PROFILES")
    write_new(output / "input-pins.json", {"manifest": manifest_pin, "files": pins, "builds": builds, "differences": differences, "offlineOnly": True})
    original = json.loads((source / "engine-upgrade-v1.json").read_bytes())
    head = json.loads((source / "head.json").read_bytes())
    report = json.loads((source / "report.json").read_bytes())
    require(len(original["upgrades"]) == 12 and head["sequence"] == m["expectedSequence"] and report["sequence"] == head["sequence"], "ORIGINAL_BOUNDARY")
    old_id, new_id = (builds[x]["engineIdentity"] for x in ("baseline", "candidate"))
    require(original["upgrades"][-1]["toEngineIdentity"] == old_id and old_id != new_id, "ORIGINAL_FINAL_IDENTITY")
    for role in ("baseline", "candidate"):
        found, _ = invoke(Path(m[role]["probePath"]), ["identity"], output, role + "-identity", 30, False)
        require(found == builds[role]["engineIdentity"], "COMPILED_ENGINE_IDENTITY")
    baseline = copy_bank(source, output, "baseline-copy", pins)
    old, old_time = invoke(Path(m["baseline"]["probePath"]), ["replay", str(baseline), old_id], output, "baseline-replay", m["maximumProbeSeconds"])
    require(old["engineIdentity"] == old_id and old["productionAuthorization"] is False, "BASELINE_RESULT")
    unchanged(baseline, pins)
    baseline_state = pin(baseline / "comparison-state.json")
    candidate = copy_bank(source, output, "candidate-copy", pins)
    proposed, _ = invoke(Path(m["candidate"]["probePath"]), ["propose", str(candidate), new_id, m["oldBinarySha256"], m["offlineApprovedAt"]], output, "candidate-offline-proposal", m["maximumProbeSeconds"])
    proposal_check(proposed, original, head, report, pins["events.jsonl"], old_id, new_id)
    shutil.copyfile(candidate / "engine-upgrade-v1.json", output / "original-twelve-bridges.json")
    # Only this detached copy's bridge file changes. This is explicitly NOT a
    # live bridge approval or assertion that production540259 has stopped.
    with (candidate / "engine-upgrade-v1.json").open("wb") as stream:
        stream.write((output / "candidate-offline-proposal.stdout").read_bytes())
        stream.flush()
        os.fsync(stream.fileno())
    new, new_time = invoke(Path(m["candidate"]["probePath"]), ["replay", str(candidate), new_id], output, "candidate-replay", m["maximumProbeSeconds"])
    require(new["engineIdentity"] == new_id and new["productionAuthorization"] is False and old["durableHead"] == new["durableHead"], "CANDIDATE_RESULT")
    unchanged(candidate, pins, True)
    candidate_state = pin(candidate / "comparison-state.json", baseline_state)
    require(json.loads((candidate / "engine-upgrade-v1.json").read_bytes()) == proposed, "OFFLINE_BRIDGE_MUTATED")
    unchanged(source, pins)
    for role in ("baseline", "candidate"):
        source_identity(m[role]["nativeSourceRoot"], m[role]["sourceFiles"])
        pin(m[role]["probePath"], m[role]["probe"])
    result = {"schemaVersion": 1, "offlineOnly": True, "productionAuthorization": False, "productionStopped": False, "sourceCodeSha256": sha(Path(__file__).read_bytes()), "manifest": manifest_pin, "baseline": old, "candidate": new, "timing": {"baseline": old_time, "candidate": new_time}, "parity": {"originalJournalHeadManifestReportBytes": True, "finalPublicEngineState": {"baseline": baseline_state, "candidate": candidate_state}, "originalTwelveBridgeEntries": True, "candidateThirteenthOfflineOnly": True}, "limitations": ["One baseline-first pair: cache warm-up and shared CPU confound speed inference.", "previousProcessStopped in the detached proposal refers only to the completed detached baseline probe, never the live production worker.", "Open-live timing includes original replay, all upgrade checks and final publication on copies; proposal/copy/hash time is separate.", "Cumulative child RSS is not a per-trial memory difference; no production latency or profitability claim."]}
    write_new(output / "result.json", result)
    print(json.dumps({"output": str(output), "resultSha256": pin(output / "result.json")["sha256"], "parity": True, "offlineOnly": True}))

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("manifest", type=Path)
    parser.add_argument("manifest_sha256")
    args = parser.parse_args()
    run(args.manifest, args.manifest_sha256)
