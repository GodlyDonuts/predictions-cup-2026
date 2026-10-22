"""Build a file-only verifier against the exact qualified R103 SDK libraries."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import subprocess


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__); p.add_argument('--source',type=Path,required=True)
    p.add_argument('--output-root',type=Path,required=True); a = p.parse_args()
    os.nice(15); Path('/proc/self/oom_score_adj').write_text('1000')
    base = Path('/tmp/moloch-generation-pins-r103-20261022')
    q = json.loads((base/'qualified.public.json').read_bytes())
    assert hashlib.sha256((base/'libmoloch_live_runtime_release.rlib').read_bytes()).hexdigest() == q['runtimeLibrarySha256']
    source = a.source.read_bytes().replace(b'\r\n',b'\n'); a.output_root.mkdir(mode=0o700,exist_ok=True)
    target = a.output_root/'verify_v2.rs'
    if target.exists(): assert target.read_bytes() == source
    else: target.write_bytes(source)
    prior = json.loads((base/'worker-release-command.json').read_bytes()); selected = []
    # These libraries contain LLVM bitcode; retain the qualified -C settings,
    # including thin LTO, rather than sending their archive objects to ld raw.
    for i, arg in enumerate(prior[:-1]):
        if arg in ['-L','--extern','-C']: selected.extend([arg,prior[i+1]])
    command = ['/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc',
        '--edition=2024',str(target),*selected,'-o',str(a.output_root/'verify-prior-not-sent-review')]
    (a.output_root/'reproduce-command.json').write_text(json.dumps(command))
    subprocess.run(command,check=True)
