#!/bin/bash
set -euo pipefail
base="$HOME/apps/moloch"
release="$base/releases/rust-session-ca5a37da"
test ! -e "$release"
test -f "$base/shared/NETWORK_DISABLED"
test -f "$base/shared/EXECUTION_STOP"
test ! -e "$base/shared/NETWORK_ENABLED"
! systemctl --user is-active --quiet moloch-capture.service
mkdir -m 700 "$release"
mkdir -m 700 "$release/bin"
for name in moloch-collector moloch-paper moloch-replay moloch-seed moloch-session moloch-source moloch-ready moloch-compare; do
 install -m 700 "$base/rust-build/target/release/$name" "$release/bin/$name"
done
(cd "$release"; sha256sum bin/* > SHA256SUMS)
install -m 600 "$base/shared"/rust-continuation-*/policy.json "$base/shared/policy.json"
install -m 600 "$base/legacy-wallet-993fd7ba2700.json" "$base/shared/legacy-wallet.json"
: > "$base/shared/empty-events.jsonl"
"$release/bin/moloch-session" ingest "$base/shared/paper-bank" "$base/shared/legacy-wallet.json" "$base/shared/policy.json" deployment-init "$base/shared/empty-events.jsonl" > "$base/shared/session-initialization.json"
python3 - <<'PY'
import json,os
from pathlib import Path
p=Path.home()/'apps/moloch/shared'
c=json.loads((p/'deployment.json').read_text())
c.update(runtime='rust',continuousPaper=False,captureCycles=15,pairsFile=str(p/'pairs.json'),paperSession={'directory':str(p/'paper-bank'),'seedSnapshotFile':str(p/'legacy-wallet.json'),'policyFile':str(p/'policy.json')})
(p/'deployment.json').write_text(json.dumps(c,indent=2)+'\n')
(p/'runtime.env').write_text('MOLOCH_NETWORK_ENABLED=0\nMOLOCH_STATE_DIR='+str(p)+'\nPATH=/usr/bin:/bin\n')
PY
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
(cd "$release"; sha256sum -c SHA256SUMS)
