"""Bounded diagnostic R26/R27 wrapper; unchanged checked book-clock algorithm."""
import argparse
import json
from pathlib import Path
import importlib.util

HERE=Path(__file__).resolve().parent
ALGORITHM_LF_SHA='1fbc599e16417778d4aa48545eb2c7af9c3073875fd4ffb26f72c0333443a34b'
PROTOCOL_SHA='d2a37f276a444d294ba4b913f61ac0dc9fc8cd496bada9db01cd67e5f9a73bd1'
OLD_SHA='ff6d775e6f90d4e08cae42442e99165188411738ea8687def119311096045bba'
INCIDENT_SHA='8c09fd1aaa589dd0e4e55da09188b5b5aa7f19c9ea3378c779155da534dc02d3'
# Bootstrap the existing checker without modifying its frozen protocol or metrics.
import hashlib
def digest(raw):return hashlib.sha256(raw).hexdigest()
raw=(HERE/'compare_book_clocks.py').read_bytes()
if digest(raw.replace(b'\r\n',b'\n'))!=ALGORITHM_LF_SHA:raise ValueError('R27_ALGORITHM_CHANGED')
spec=importlib.util.spec_from_file_location('_r27_checked_clocks',HERE/'compare_book_clocks.py')
C=importlib.util.module_from_spec(spec);spec.loader.exec_module(C)

def generations(packet):
    raw=packet.small(packet.name('fundamental.original.jsonl'),16*1024**2)
    previous='0'*64;seq=0;gens=[];last=None
    for line in raw.splitlines(keepends=True):
        row,_=C.V.sealed(line)
        C.need(row['sequence']==seq+1 and row['previous']==previous,'FUNDAMENTAL_CHAIN')
        C.need(last is None or row['atMs']>=last,'FUNDAMENTAL_CLOCK')
        seq=row['sequence'];previous=row['hash'];last=row['atMs']
        if row['event']['kind']=='review_generation':
            g=row['event']['generation'];prior=gens[-1]['generationSha256'] if gens else '0'*64
            C.need(g['request']['previousGenerationSha256']==prior,'GENERATION_CHAIN')
            gens.append({'generationSha256':g['generationSha256'],'atMs':row['atMs'],'rowSha256':row['hash']})
    return raw,gens

def run(a):
    declaration_raw=Path(a.protocol).read_bytes()
    C.need(a.protocol_sha256==PROTOCOL_SHA and digest(declaration_raw)==PROTOCOL_SHA,'R27_PROTOCOL')
    declaration=C.V.load(declaration_raw)
    C.need(declaration['windowMs']==300000 and declaration['policyMaximumBookAgeMs']==10000 and declaration['oldOutcomeAlreadyInspected'] is True,'R27_PROTOCOL_FIELDS')
    C.need(a.old_sha256==OLD_SHA and a.incident_sha256==INCIDENT_SHA,'R27_FIXED_INPUTS')
    packets=[]
    try:
        old=C.Packet(a.old_packet,a.old_sha256);packets.append(old)
        incident=C.Packet(a.incident_packet,a.incident_sha256);packets.append(incident)
        original,g5=generations(incident);oldraw,g4=generations(old)
        C.need(len(g5)==5 and len(g4)==4 and original.startswith(oldraw),'R27_ORIGINAL_GENERATIONS')
        start,text=C.progress(old,'first-trading-progress.original.json',651324)
        C.need(text==declaration['oldWindowStartUtc'],'R27_OLD_START')
        result={'protocolRawSha256':PROTOCOL_SHA,'algorithmLfSha256':ALGORITHM_LF_SHA,'wrapperRawSha256':digest(Path(__file__).read_bytes()),
            'oldOutcomeAlreadyInspected':True,'confirmatory':False,'comparisonPerformed':False,'new':None,
            'old':C.measure(old,start,text,651324,{'zipSha256':old.expected,**old.original_pin('first-trading-progress.original.json')}),
            'incident':{'zipSha256':incident.expected,'originalFundamental':incident.original_pin('fundamental.original.jsonl'),'generations':g5},
            'oldGenerations':g4,'limitations':declaration['limitations']+['Admitted native sealed records do not prove underlying original source-body membership.','Root-verified rollout provenance is caller-pinned packet evidence, not independently observed process execution.'],
            'executionAuthority':False,'profitabilityEstablished':False,'causalBenefitEstablished':False}
        if not a.old_only:
            new=C.Packet(a.new_packet,a.new_sha256);packets.append(new)
            rollout=new.json('production-rollout.original.json')
            C.need(type(rollout.get('newPid')) is int and rollout['newPid']>0 and rollout['newPid']!=651324,'R27_ROLLOUT_PID')
            pid=rollout['newPid'];start,text=C.progress(new,'first-trading-progress.original.json',pid)
            current,current_g=generations(new)
            C.need(current.startswith(original) and len(current_g)==5 and current_g==g5,'R27_FIVE_GENERATION_PREFIX')
            C.need(all(g['atMs']*1000000<=start for g in current_g),'R27_GENERATION_BEFORE_WINDOW')
            result['new']=C.measure(new,start,text,pid,{'zipSha256':new.expected,**new.original_pin('first-trading-progress.original.json')})
            result['new']['productionRolloutOriginal']=new.original_pin('production-rollout.original.json')
            result['new']['generations']=current_g;result['new']['wholeOriginalFundamentalPrefixRetained']=True
            result['comparisonPerformed']=True
        for p in packets:p.finish()
        C.need(digest(Path(a.protocol).read_bytes())==PROTOCOL_SHA,'R27_PROTOCOL_CHANGED')
        return result
    finally:
        for p in packets:p.z.close()

def main():
    p=argparse.ArgumentParser()
    for n in ('old-packet','old-sha256','incident-packet','incident-sha256','protocol','protocol-sha256','output'):p.add_argument('--'+n,required=True)
    p.add_argument('--new-packet');p.add_argument('--new-sha256');p.add_argument('--old-only',action='store_true');a=p.parse_args()
    C.need((a.old_only and a.new_packet is None and a.new_sha256 is None) or (not a.old_only and a.new_packet is not None and a.new_sha256 is not None),'R27_EXPLICIT_MODE')
    out=Path(a.output);C.need(not out.exists(),'R27_OUTPUT_EXISTS');result=run(a)
    with out.open('x',encoding='utf-8',newline='\n') as f:json.dump(result,f,indent=2,allow_nan=False);f.write('\n')
if __name__=='__main__':main()
