"""Run the direct joint-target sequential research search on retained inputs.

Optional NumPy/SciPy are required. No network client, approval or order exists
here. Original clocks and dated/invalidation flags remain private input data;
every book is explicitly conditional future liquidity, never current authority.
"""
import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from moloch.tartarus.sequential_frontier import solve


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--input-directory", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--seconds", type=float, default=30)
    p.add_argument("--field", default="dated_flat_anchor")
    p.add_argument("--extra-mean-floor", action="append", default=[],
                   help="Additional explicit monetary-floor sweep; use = before a negative value")
    a = p.parse_args(); d = a.input_directory.resolve(strict=True)
    load = lambda n: json.loads((d / n).read_text(encoding="utf-8"))
    fin = load("financial-lots-with-inherited-capacity-v3-574174.private.json")
    registry = load("full-universe-expanded/census.private.json")["rows"]
    books = load("dated-sig-depth.private.json")["books"]
    views = load("global-five-model.private.json")
    pairs = [(m["id"], a.field) for m in views["model"]["probabilityModels"]]
    outputs = []
    cases = [("mean", None), ("rank", "0.000000"),
                             ("rank", "-10000.000000"), ("rank", None),
                             ("robust_rank", None)]
    cases.extend(("rank", floor) for floor in a.extra_mean_floor)
    for objective, floor in cases:
        result = solve(fin, registry, books, views["model"], views["fields"],
                       target_pairs=pairs, objective=objective, seconds=a.seconds,
                       minimum_expected_change=floor)
        outputs.append(result)
        a.output.write_text(json.dumps(outputs, indent=2) + "\n", encoding="utf-8")
        audit = result.get("independentExactAudit", {})
        print(json.dumps({"objective": objective, "meanFloor": floor,
            "status": result["status"], "solverStatus": result["solverStatus"],
            "seconds": result["seconds"], "writes": len(result.get("legs", [])),
            "averageStrictMassPpm": result.get("selectedAverageStrictMassPpm"),
            "minimumStrictMassPpm": result.get("selectedMinimumStrictMassPpm"),
            "weakestExpectedChange": min((m["expectedChangeMicrosNumerator"] / 1e12
                for m in audit.get("models", [])), default=None),
            "exactRejection": result.get("rejectionReason"),
            "executionAuthority": False}), flush=True)


if __name__ == "__main__":
    main()
