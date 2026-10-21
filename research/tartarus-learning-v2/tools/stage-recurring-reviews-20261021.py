"""Stage V3 review opt-in only; no account request, ledger write or reload."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['base-control', 'output', 'review-helper']:
        p.add_argument('--' + name, required=True, type=Path)
    p.add_argument('--base-sha256', required=True)
    p.add_argument('--exchanges', required=True)
    a = p.parse_args()
    raw = a.base_control.read_bytes()
    assert sha(raw) == a.base_sha256 and not a.output.exists()
    control = json.loads(raw)
    original = json.loads(raw)
    selected = set(a.exchanges.split(','))
    assert selected and '' not in selected
    a.output.mkdir(mode=0o700)
    reviews, trust, changed = [], [], []
    for old in control['fundamentalPolicy']['reviews']:
        old_raw = Path(old['path']).read_bytes()
        assert sha(old_raw) == old['fileSha256']
        review = json.loads(old_raw)
        if review['exchangeId'] in selected:
            assert review['version'] == 2 and review.get('existingOrigin') is None
            assert review['sameSideTopUps']['version'] == 1
            review['sameSideTopUps']['version'] = 3
            target = a.output / (review['exchangeId'] + '-review.json')
            data = json.dumps(review, separators=(',', ':')).encode()
            target.write_bytes(data)
            item = dict(old, path=str(target), fileSha256=sha(data))
            changed.append(review['exchangeId'])
        else:
            item = old
        for source in item['sourceFiles']:
            assert sha(Path(source['path']).read_bytes()) == source['sha256']
        trust.append(subprocess.check_output([str(a.review_helper), 'hash-review', item['path']], text=True).strip())
        reviews.append(item)
    assert set(changed) == selected and len(changed) == len(selected)
    authority = a.output / 'authority.owner-reviewed.json'
    authority.write_text(json.dumps(dict(version=1, trustedReviewSha256=trust)), encoding='utf-8')
    control['fundamentalPolicy'] = dict(control['fundamentalPolicy'], reviews=reviews,
        authorityPath=str(authority), authoritySha256=sha(authority.read_bytes()), allowInitialization=False)
    assert {k: v for k, v in control.items() if k != 'fundamentalPolicy'} == {k: v for k, v in original.items() if k != 'fundamentalPolicy'}
    target = a.output / 'control.owner-reviewed.json'
    target.write_text(json.dumps(control, indent=2) + '\n', encoding='utf-8')
    result = dict(controlPath=str(target), controlSha256=sha(target.read_bytes()), exchanges=sorted(changed),
        onlyChange='sameSideTopUps.version 1 -> 3; authority/file hashes', monetaryCapsChanged=False,
        reviewClocksRenewed=False, forecastOrSourcesChanged=False, accountRequests=0, reloadSubmitted=False)
    (a.output / 'staged.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))

if __name__ == '__main__':
    main()
