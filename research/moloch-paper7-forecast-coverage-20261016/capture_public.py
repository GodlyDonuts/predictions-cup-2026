"""Finite public HTML capture; no SIG requests, credentials, APIs or execution."""
import concurrent.futures
import datetime as dt
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys

ORIGIN = "https://www.callthemap.com"


def capture(folder, route):
    if not re.fullmatch(r"/(?:senate|governors)(?:/[a-z-]+)?|/methodology", route):
        raise ValueError("public_route")
    name = route.strip("/").replace("/", "-") + ".html"
    dest = folder / name
    if dest.exists():
        raise ValueError("immutable_capture_exists")
    started = dt.datetime.now(dt.timezone.utc).isoformat()
    curl = shutil.which("curl.exe") or shutil.which("curl")
    if not curl:
        raise ValueError("curl_missing")
    result = subprocess.run([curl, "--fail", "--silent", "--show-error", "--max-time", "25",
                             "--proto", "=https", "--max-filesize", "4000000",
                             "--dump-header", str(folder / (name + ".headers")),
                             "--output", str(dest), ORIGIN + route], capture_output=True)
    received = dt.datetime.now(dt.timezone.utc).isoformat()
    item = {"file": name, "url": ORIGIN + route, "requestStartedAt": started, "receivedAt": received}
    if result.returncode:
        return {**item, "error": result.stderr.decode("utf-8", errors="replace"), "exitCode": result.returncode}
    raw = dest.read_bytes()
    headers = (folder / (name + ".headers")).read_bytes()
    lines = headers.decode("iso8859-1").splitlines()
    statuses = [int(m[1]) for line in lines if (m := re.match(r"HTTP/\S+\s+(\d{3})", line))]
    if not statuses or statuses[-1] != 200:
        return {**item, "error": "public_html_status", "status": statuses[-1] if statuses else None}
    fields = dict(line.split(": ", 1) for line in lines if ": " in line)
    return {**item, "status": 200, "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest(),
            "headersSha256": hashlib.sha256(headers).hexdigest(),
            "httpDate": fields.get("date", fields.get("Date")),
            "lastModified": fields.get("last-modified", fields.get("Last-Modified"))}


def run(folder):
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    pins = []
    for route in ["/senate", "/governors", "/methodology"]:
        pins.append(capture(folder, route))
    routes = set()
    for office in ["senate", "governors"]:
        raw = (folder / (office + ".html")).read_text(encoding="utf-8")
        routes.update(re.findall(r'href="(/' + office + r'/[a-z-]+)"', raw))
    if not 60 <= len(routes) <= 75:
        raise ValueError("finite_race_count")
    # Two public HTTP requests maximum in flight; each URL is an original public page link.
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        pins.extend(pool.map(lambda route: capture(folder, route), sorted(routes)))
    manifest = {"schemaVersion": 1, "kind": "public_published_html_original_capture", "sources": pins,
                "accountRequests": 0, "undocumentedApiRequests": 0}
    (folder / "source-pins.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"captured": sum(x.get("status") == 200 for x in pins), "attempted": len(pins),
                      "manifestSha256": hashlib.sha256((folder / "source-pins.json").read_bytes()).hexdigest()}))


if __name__ == "__main__":
    run(sys.argv[1])
