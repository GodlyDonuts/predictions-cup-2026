#!/bin/bash
# Install only an explicitly supplied, audited paper wallet and frozen policy.
set -euo pipefail
if [ "$#" != 4 ]; then echo 'Usage: install-rust-disabled.sh RELEASE_ID POLICY SEED PAIRS' >&2; exit 2; fi
case "$1" in ''|*[!a-zA-Z0-9_-]*) echo 'Invalid release ID' >&2; exit 2;; esac
base="$HOME/apps/moloch"
release="$base/releases/$1"
policy_source=$(readlink -f "$2")
seed_source=$(readlink -f "$3")
pairs_source=$(readlink -f "$4")
test ! -e "$release"
test -f "$policy_source" && test -f "$seed_source" && test -f "$pairs_source"
test -f "$base/shared/NETWORK_DISABLED" && test -f "$base/shared/EXECUTION_STOP"
test ! -e "$base/shared/NETWORK_ENABLED"
! systemctl --user is-active --quiet moloch-capture.service
python3 - <<'PY'
import json
from pathlib import Path
p=Path.home()/'apps/moloch/shared'
c=json.loads((p/'deployment.json').read_text())
assert c['networkEnabled'] is False and c['tradingEnabled'] is False and c['readsPerMinute']==100
assert not (p/'credentials/sig-api.key').exists()
PY
"$base/rust-build/target/release/moloch-ready" scope-check "$pairs_source" "$policy_source"
mkdir -m 700 "$release"
mkdir -m 700 "$release/bin"
for name in moloch-collector moloch-paper moloch-replay moloch-seed moloch-session moloch-source moloch-ready moloch-compare; do
 install -m 700 "$base/rust-build/target/release/$name" "$release/bin/$name"
done
(cd "$release"; sha256sum bin/* > SHA256SUMS; sha256sum -c SHA256SUMS)
: > "$release/empty-events.jsonl"
# Existing banks must match the sealed seed, policy and engine identity. No reset.
"$release/bin/moloch-session" ingest "$base/shared/paper-bank" "$seed_source" "$policy_source" deployment-init "$release/empty-events.jsonl" > "$release/session-audit.json"
for spec in "$policy_source:policy.json" "$seed_source:legacy-wallet.json" "$pairs_source:pairs.json"; do
 source_file=${spec%:*}; target_file="$base/shared/${spec##*:}"
 if [ "$source_file" != "$(readlink -f "$target_file")" ]; then install -m 600 "$source_file" "$target_file"; fi
done
python3 - <<'PY'
import json
from pathlib import Path
p=Path.home()/'apps/moloch/shared'
c=json.loads((p/'deployment.json').read_text())
c.update(runtime='rust',continuousPaper=False,captureCycles=15,pairsFile=str(p/'pairs.json'),paperSession={'directory':str(p/'paper-bank'),'seedSnapshotFile':str(p/'legacy-wallet.json'),'policyFile':str(p/'policy.json')})
(p/'deployment.json').write_text(json.dumps(c,indent=2)+'\n')
(p/'runtime.env').write_text('MOLOCH_NETWORK_ENABLED=0\nMOLOCH_STATE_DIR='+str(p)+'\nPATH=/usr/bin:/bin\n')
PY
MOLOCH_STATE_DIR="$base/shared" "$release/bin/moloch-ready" > "$release/readiness-audit.json"
cat > "$base/check" <<'CHECK'
#!/bin/sh
export MOLOCH_STATE_DIR="$HOME/apps/moloch/shared"
exec "$HOME/apps/moloch/current/bin/moloch-ready"
CHECK
chmod 700 "$base/check"
cat > "$HOME/.config/systemd/user/moloch-capture.service" <<'UNIT'
[Unit]
Description=Moloch Rust paper collector (disabled deployment)
ConditionPathExists=%h/apps/moloch/shared/NETWORK_ENABLED
[Service]
Type=simple
WorkingDirectory=%h/apps/moloch/current
EnvironmentFile=%h/apps/moloch/shared/runtime.env
ExecStart=%h/apps/moloch/current/bin/moloch-collector run
Restart=no
UMask=0077
NoNewPrivileges=yes
PrivateTmp=yes
[Install]
WantedBy=default.target
UNIT
ln -s "$release" "$base/current-rust-next"
mv -Tf "$base/current-rust-next" "$base/current"
systemctl --user daemon-reload
"$base/check"
