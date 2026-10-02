"""Offline integrity and structural verification; no network or order operations."""
import collections,gzip,hashlib,json
from decimal import Decimal
from pathlib import Path
ROOT=Path(__file__).resolve().parent
sha=lambda b:hashlib.sha256(b).hexdigest()
def read(name):
    p=ROOT/name
    if p.exists():return p.read_bytes()
    return gzip.decompress((ROOT/(name+'.gz')).read_bytes())
def load(name):return json.loads(read(name))
def main():
    m=load('bundle_manifest.json')
    for name,e in m['files'].items():
        p=ROOT/name;assert p.resolve().is_relative_to(ROOT),name
        b=p.read_bytes();assert len(b)==e['bytes'] and sha(b)==e['sha256'],name
        assert hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest()==e['git_blob_sha1'],name
        if name.endswith('.gz'):assert b[3]==0 and b[4:8]==b'\0'*4,name
    for e in load('source_manifest.json')['original_files']:
        if e['published_as']:
            b=read(e['original_name']);assert len(b)==e['bytes'] and sha(b)==e['sha256'],e['original_name']
    ids=load('cup_ui_identity_map_2026-10-02.json');bulk=load('cup_bulk_leads.json');joins=load('join_records.json');ranked=load('conditional_crossvenue_leads.json');un=load('unmatched_and_ambiguous.json');suppressed=load('suppressed_side_observations.json')
    assert len(ids['markets'])==len(bulk['exchanges'])==len(joins)==237
    assert len({str(r['market_id']) for r in ids['markets']})==237
    assert {str(r['market_id']) for r in ids['markets']}=={str(r['marketId']) for r in bulk['exchanges']}
    assert sum(r['live_reverified_today'] for r in ids['markets'])==49
    assert sha(read('public_market_inventory_2026-10-01_184851.json'))==ids['prior_catalog_sha256']
    assert sum(r['conditional_reference_matches']==1 for r in joins)==226
    assert len(un['unmatched'])==11 and len(un['ambiguous'])==0
    assert len(ranked)==451 and len(suppressed)==1 and suppressed[0]['conditional_bid_minus_cup_ask_gap'] is None
    assert sum(Decimal(r['conditional_bid_minus_cup_ask_gap'])>0 for r in ranked)==99
    for r in ranked:
        assert Decimal(r['reference_bid'])-Decimal(r['cup_stale_ask'])==Decimal(r['conditional_bid_minus_cup_ask_gap'])
        assert r['cup_quantity'] is None and r['cup_source_quote_age_seconds'] is None and r['reference_source_quote_age_seconds'] is None
        if r['side']=='NO':assert Decimal(r['cup_stale_ask'])==1-Decimal(r['cup_best_yes_bid']) and r['cup_ask_is_derived']
    assert len(load('top10_candidates.json'))==10 and len(load('ui_check_shortlist.json'))==5
    assert len(load('cup_separate_party_pair_diagnostics.json'))==117
    print(f"PASS: {len(m['files'])} manifest-listed file hashes, gzip/source integrity, 237 identities, 49 reverified, 226 conditional matches, 451 ranked sides, 99 positive stale gaps")
    print('Historical research only; no settlement equivalence, current executable opportunity or current account state is asserted')
if __name__=='__main__':main()
