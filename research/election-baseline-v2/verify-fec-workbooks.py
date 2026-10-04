"""Check the actual named final-stage columns against the model's override facts."""
from pathlib import Path
import hashlib
import json
import openpyxl

base = Path(__file__).resolve().parent
manifest = json.loads((base / "source-manifest.json").read_text())
overrides = {r["key"]: r for r in json.loads((base / "final-stage-overrides.json").read_text())}
specs = [
    ("2020:GA", "federalelections2020.xlsx", "12. US Senate Results by State", "GE RUNOFF ELECTION VOTES (GA, LA)", {"Jon Ossoff": "JON OSSOFF", "David A. Perdue": "DAVID A. PERDUE"}),
    ("2016:LA", "federalelections2016.xlsx", "2016 US Senate Results by State", "GE RUNOFF ELECTION VOTES (LA)", {"John Kennedy": "JOHN KENNEDY", "Foster Campbell": "FOSTER CAMPBELL"}),
]
def canonical(s):
    return " ".join(str(s or "").replace(".", "").split()).upper()
for key, filename, sheet, final_column, names in specs:
    source = next(s for s in manifest["sources"] if s["filename"] == filename)
    data = (base / "sources" / filename).read_bytes()
    assert len(data) == source["bytes"] and hashlib.sha256(data).hexdigest() == source["sha256"]
    workbook = openpyxl.load_workbook(base / "sources" / filename, read_only=True, data_only=True)
    iterator = iter(workbook[sheet].values)
    header = [str(v or "").strip() for v in next(iterator)]
    name_first, name_last = header.index("CANDIDATE NAME (First)"), header.index("CANDIDATE NAME (Last)")
    party_index, state_index, votes_index = header.index("PARTY"), header.index("STATE ABBREVIATION"), header.index(final_column)
    actual = {}
    wanted = {canonical(n): target for n, target in names.items()}
    for row in iterator:
        name = canonical(str(row[name_first] or "") + " " + str(row[name_last] or ""))
        if name in wanted and row[state_index] == key.split(":")[1]:
            assert name not in actual and isinstance(row[votes_index], int)
            actual[name] = (row[votes_index], row[party_index])
    assert len(actual) == len(wanted)
    for name, target in wanted.items():
        expected = next(c for c in overrides[key]["candidates"] if c["name"] == target)
        assert actual[name] == (expected["votes"], "R" if expected["party"] == "REPUBLICAN" else "D")
    assert sum(x[0] for x in actual.values()) == overrides[key]["total"]
    workbook.close()
print(json.dumps({"verifiedOverrides": [s[0] for s in specs], "finalColumnNamesVerified": True, "sourceHashesVerified": True}))
