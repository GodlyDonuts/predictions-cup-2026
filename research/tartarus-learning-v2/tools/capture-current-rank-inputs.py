"""Finite file-only portfolio/review capture from the existing official worker.

No account client, order, retirement, approval or timestamp renewal. A matched
native report is research evidence; physical dispatch must reconcile again.
"""
import argparse
from datetime import datetime, timezone
from hashlib import sha256
import json
import os
from pathlib import Path
import sys
import time


def digest(raw):
    return sha256(raw).hexdigest()


def read_research_pair(root, bank, registry, at):
    from moloch.tartarus.current_portfolio import compile_current
    view_raw = (root / 'tartarus-current-native-state.json').read_bytes()
    view = json.loads(view_raw)
    age = (at - datetime.fromisoformat(view['writtenAt'].replace('Z', '+00:00'))).total_seconds()
    if not 0 <= age < 90:
        raise ValueError('CURRENT_RESEARCH_VIEW_AGE')
    report_raw = (bank / 'report.json').read_bytes()
    # This retained pair must agree with ITSELF, not a subsequently advancing
    # writer head. The compiler verifies seals, exact heads, cash, FIFO and fills.
    compiled = compile_current(report_raw, view_raw,
        expected_report_sha256=digest(report_raw), expected_view_sha256=digest(view_raw),
        registry=registry, conditional_future_legs=128)
    return view_raw, report_raw, compiled


def run(args):
    os.umask(0o077)
    sys.path.insert(0, str(args.source))
    from moloch.tartarus.current_portfolio import compile_current
    root = args.runtime
    bank = root / "bank-all-markets-v1"
    deadline = time.monotonic() + args.wait_seconds
    args.output.mkdir(mode=0o700, parents=True, exist_ok=False)
    pins = json.loads((args.source / "package-pins.json").read_bytes())
    for name, expected in pins["files"].items():
        assert digest((args.source / name).read_bytes()) == expected, name
    registry = json.loads((args.source / "originals/full-universe-expanded/census.private.json").read_bytes())["rows"]
    process = Path("/proc") / str(args.pid)
    process_start = (process / "stat").read_text().rsplit(")", 1)[1].split()[19]
    assert digest((process / "exe").read_bytes()) == args.binary_sha256, "WORKER_BINARY_CHANGED"
    last = "waiting_for_matching_head"
    while time.monotonic() < deadline:
        try:
            assert (process / "stat").read_text().rsplit(")", 1)[1].split()[19] == process_start, "WORKER_BINARY_CHANGED"
            assert not (root / "EXECUTION_STOP").exists(), "EXECUTION_STOP_PRESENT"
            view_raw, report_raw, compiled = read_research_pair(root, bank, registry, datetime.now(timezone.utc))
            view = json.loads(view_raw)
            assert compiled["financial"] is not None, compiled["unavailableReasons"]
            break
        except ValueError as error:
            if str(error) not in ('MATCHED_NATIVE_BOUNDARY_REQUIRED', 'CURRENT_FINANCIAL_BINDING', 'CURRENT_RESEARCH_VIEW_AGE'):
                raise
            last = str(error)
            time.sleep(0.15)
        except (AssertionError, FileNotFoundError, json.JSONDecodeError) as error:
            last = str(error)
            if last in ("WORKER_BINARY_CHANGED", "EXECUTION_STOP_PRESENT"):
                raise
            time.sleep(0.15)
    else:
        raise RuntimeError("MATCHED_PORTFOLIO_TIMEOUT: " + last)
    # The hot-reload state identifies the actual current fundamental control.
    state_path = args.reference / "state.json"
    state_raw = state_path.read_bytes()
    state = json.loads(state_raw)
    assert state["generationSha256"] == view["generationSha256"], "GENERATION_CHANGED"
    control_raw = Path(state["controlPath"]).read_bytes()
    assert digest(control_raw) == state["controlSha256"], "CONTROL_PIN"
    control = json.loads(control_raw)
    reviews = []
    for row in control["fundamentalPolicy"]["reviews"]:
        raw = Path(row["path"]).read_bytes()
        assert digest(raw) == row["fileSha256"], "REVIEW_PIN"
        for source in row["sourceFiles"]:
            assert digest(Path(source["path"]).read_bytes()) == source["sha256"], "REVIEW_SOURCE_PIN"
        reviews.append({"bodyText": raw.decode(), "sha256": digest(raw)})
    assert state_raw == state_path.read_bytes(), "GENERATION_CHANGED_DURING_CAPTURE"
    now = datetime.now(timezone.utc).isoformat()
    projection = {"capturedAt": now, "controlSha256": digest(control_raw), "reviews": reviews,
                  "reviewClocksRenewed": False, "executionAuthority": False}
    files = {"report.original.json": report_raw, "native-view.original.json": view_raw,
             "active-fourteen-reviews.private.json": (json.dumps(projection, indent=2) + "\n").encode(),
             "compiled-current.private.json": (json.dumps(compiled, indent=2) + "\n").encode()}
    for name, raw in files.items():
        (args.output / name).write_bytes(raw)
    financial = compiled["financial"]
    result = {"capturedAt": now, "status": compiled["status"], "nativeSequence": compiled["nativeSequence"],
              "viewWrittenAt": view["writtenAt"], "cash": financial["cash"], "ownedCost": financial["ownedCost"],
              "reservedCash": financial["reservedCash"], "heldMarkets": len(financial["positions"]),
              "fillCount": view["account"]["fillCount"], "knownPhysicalOrders": len(view["knownUnclosedAcknowledgments"]),
              "reviewCount": len(reviews), "generationSha256": view["generationSha256"],
              "sourceCommit": pins["sourceCommit"], "originalFileSha256": {n: digest(b) for n, b in files.items()},
              "newAccountClients": 0, "networkRequests": 0, "approvalPublished": False,
              "sourceClocksRenewed": False, "executionAuthority": False}
    (args.output / "capture.public.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result), flush=True)


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source", type=Path, required=True)
    p.add_argument("--runtime", type=Path, required=True)
    p.add_argument("--reference", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--pid", type=int, required=True)
    p.add_argument("--binary-sha256", required=True)
    p.add_argument("--wait-seconds", type=int, default=180)
    run(p.parse_args())
