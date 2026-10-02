"""Reproduce the original screen offline using pinned bytes and frozen analysis time.
Original scripts are copied into a temporary directory; their published bytes are
not edited. Only input/output locations and the analysis clock are adapted.
"""
import argparse,gzip,hashlib,json,subprocess,sys,tempfile
from pathlib import Path
from verify_bundle import ROOT,read,load,sha,main as verify

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--reference-dir',type=Path,default=ROOT.parent/'public_reference_2026-10-02')
    args=parser.parse_args();verify()
    source=load('source_manifest.json');pin=source['pinned_repository_inputs']['public_reference']
    packed=(args.reference_dir/'reference.json.gz').read_bytes();ref=gzip.decompress(packed);books=(args.reference_dir/'orderbook_snapshots.json').read_bytes()
    assert sha(packed)==pin['reference_gzip_sha256'] and sha(ref)==pin['reference_json_sha256'] and sha(books)==pin['orderbook_snapshots_sha256'],'Pinned reference input changed'
    with tempfile.TemporaryDirectory() as tmp:
        base=Path(tmp);out=base/'crossvenue_screen_2026-10-02';out.mkdir();r=base/'public_election_reference_2026-10-02';r.mkdir()
        (base/'cup_ui_identity_map_2026-10-02.json').write_bytes(read('cup_ui_identity_map_2026-10-02.json'))
        (out/'cup_bulk_leads.json').write_bytes(read('cup_bulk_leads.json'));(r/'reference.json').write_bytes(ref);(r/'orderbook_snapshots.json').write_bytes(books)
        code=(ROOT/'build_screen.py').read_text()
        old="BASE=Path('/workspace/shared/predictions_cup'); OUT=BASE/'crossvenue_screen_2026-10-02'"
        assert code.count(old)==1 and code.count('NOW=datetime.now(timezone.utc)')==1
        code=code.replace(old,f"BASE=Path({str(base)!r}); OUT=BASE/'crossvenue_screen_2026-10-02'")
        code=code.replace('NOW=datetime.now(timezone.utc)',f"NOW=datetime.fromisoformat({source['original_screen_generated_at_utc']!r})")
        # Preserve the historical provenance string while redirecting local I/O.
        old="str(BASE/'first_place_strategy_requirements_2026-10-01.md')"
        assert code.count(old)==1
        code=code.replace(old,repr('/workspace/shared/predictions_cup/first_place_strategy_requirements_2026-10-01.md'))
        (out/'build_screen.py').write_text(code)
        subprocess.run([sys.executable,str(out/'build_screen.py')],check=True,capture_output=True,text=True)
        report=(ROOT/'write_report.py').read_text();old="P=Path('/workspace/shared/predictions_cup/crossvenue_screen_2026-10-02')";assert report.count(old)==1
        (out/'write_report.py').write_text(report.replace(old,f'P=Path({str(out)!r})'))
        subprocess.run([sys.executable,str(out/'write_report.py')],check=True,capture_output=True,text=True)
        count=0
        for e in source['original_files']:
            name=e['original_name']
            if e['original_source_group']!='screen' or name in ['cup_bulk_leads.json','build_screen.py','write_report.py']:continue
            b=(out/name).read_bytes();assert len(b)==e['bytes'] and sha(b)==e['sha256'],f'Output differs: {name}'
            count+=1
        print(f'PASS: {count} original screen outputs reproduced byte-for-byte, including the omitted duplicate CSV and all-side JSON')
    print('No network calls; this verifies deterministic transforms, not independent truth of source observations')
if __name__=='__main__':main()
