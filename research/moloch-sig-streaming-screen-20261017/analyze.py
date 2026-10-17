"""Fixed, bounded SIG-only shadow screen. No network or execution authority."""
import argparse
import collections
from dataclasses import replace
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import stat
import sys

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]
NS=1000000
UNIT=1000000
HORIZON=60000
SLACK=3000
PROTOCOL_SHA='df2cf12b6f3e2e783b7f412d5ad45eeb3e0bd7dc5fa2fb52257681b154bc80d2'
FEATURES=('micropriceMinusMidpoint','imbalance','spread','midpointDelta1s','midpointDelta5s','midpointDelta30s')

def need(ok,code):
    if not ok:raise ValueError('SIG_SCREEN_'+code)
def sha(raw):return hashlib.sha256(raw).hexdigest()
def compact(v):return json.dumps(v,sort_keys=True,separators=(',',':'),allow_nan=False).encode()
def checked_module(path,expected,name):
    raw=path.read_bytes();need(sha(raw.replace(b'\r\n',b'\n'))==expected,'DEPENDENCY_CHANGED')
    spec=importlib.util.spec_from_file_location(name,path);module=importlib.util.module_from_spec(spec);sys.modules[name]=module
    # Execute checked bytes, never a stale pyc or manifest-selected code path.
    exec(compile(raw,str(path),'exec'),module.__dict__);return module
NATIVE=checked_module(ROOT/'research/moloch-owned-fill-markouts-20261015/analyze.py','f1db3b56bf701a0ded0c84486c04f805122deb0342fe196af17113e566e4ba39','_sig_screen_native')
MATH=checked_module(ROOT/'moloch/external-data/crossvenue_economics.py','7207506a561a26a006338679c7d4908c4b4e81817204899063bb1c69778a2542','_sig_screen_math')

def group_holdout(group):return int(sha(group.encode()),16)%5==0
class Pin:
    def __init__(self,base,spec,maximum):
        need(isinstance(spec,dict) and set(spec)=={'path','sha256','bytes'},'PIN_SCHEMA')
        self.path=(base/spec['path']).absolute();self.expected=spec
        need(type(spec['bytes']) is int and 0<spec['bytes']<=maximum,'PIN_BOUND');self.before=self.verify()
    def verify(self):
        m=self.path.lstat();need(stat.S_ISREG(m.st_mode) and m.st_nlink==1,'PIN_REGULAR_SINGLE_LINK')
        h=hashlib.sha256();n=0
        with self.path.open('rb') as f:
            for b in iter(lambda:f.read(1024*1024),b''):h.update(b);n+=len(b)
        after=self.path.stat();identity=lambda s:(s.st_dev,s.st_ino,s.st_size,s.st_mtime_ns,s.st_ctime_ns)
        need(identity(m)==identity(after) and n==self.expected['bytes'] and h.hexdigest()==self.expected['sha256'],'PIN_CHANGED')
        return identity(after)
    def finish(self):need(self.verify()==self.before,'PIN_REPLACED')
    def json(self):return json.loads(self.path.read_bytes(),object_pairs_hook=MATH.unique_object)
    def public(self):return {k:self.expected[k] for k in ('bytes','sha256')}

class Pacer:
    """One virtual serialized queue, zero HTTP duration, no historical load proof."""
    def __init__(self):self.end=0;self.reads=collections.deque();self.writes=collections.deque()
    def packet(self,at,activation=0):
        reads=collections.deque(self.reads);writes=collections.deque(self.writes);end=max(at,self.end)
        for kind,limit,gap in [('read',100,600)]*7+[('write',24,2500)]:
            q=reads if kind=='read' else writes
            end=max(end,at+activation if kind=='write' else at)
            if q:end=max(end,q[-1]+gap)
            # First read also pays a complete spacing interval, conservatively.
            if kind=='read':end=max(end,at+600)
            while q and q[0]<=end-60000:q.popleft()
            if len(q)>=limit:
                end=max(end,q[0]+60000)
                while q and q[0]<=end-60000:q.popleft()
            q.append(end)
        if end-at>10000:return None
        self.end=end;self.reads=reads;self.writes=writes;return end

class Screen:
    def __init__(self,groups=None):
        self.groups=groups or {};self.history={};self.books={};self.versions={};self.coverage={};self.trades={};self.trade_ids={};self.trade_fifo=collections.deque()
        self.blocks={};self.pending={};self.examples=[];self.counts=collections.Counter();self.pacer=Pacer();self.own={};self.own_acks={};self.own_fill_ids={};self.last=0;self.first=None
    def invalidate(self,ex,reason):
        for k in list(self.history):
            if k[2]==ex:
                self.history.pop(k,None);self.books.pop(k,None);self.versions.pop(k,None);self.trades.pop(k,None);self.coverage.pop(k,None)
                if k in self.pending:self.counts['censored_'+reason]+=1;self.pending.pop(k)
    def advance(self,at):
        need(at>=self.last,'CLOCK_ROLLBACK');self.last=at
        if self.first is None:self.first=at
        # Retain immutable public identities for this finite suffix, fail at the
        # declared bound rather than permit an evicted ID to renew its clocks.
        # Due exits are admitted before newly observed entry nominations.
        for key,p in list(self.pending.items()):
            if p['stage']=='hold' and p['target']<=at:
                ready=self.pacer.packet((p['target']+NS-1)//NS)
                if ready is None:self.counts['censored_exit_virtual_budget']+=1;self.pending.pop(key);continue
                p.update(stage='exit',ready=ready*NS,deadline=(ready+SLACK)*NS)
            if p.get('deadline',at)>=at:continue
            self.counts['censored_'+p['stage']+'_no_timely_book']+=1;self.pending.pop(key)
    def adjusted(self,b):
        bids=list(b.bids);asks=list(b.asks)
        for own in self.own.values():
            if own['exchange']!=b.exchange:continue
            levels=bids if own['yesBid'] else asks
            for i,(price,n) in enumerate(levels):
                if price==own['price']:levels[i]=(price,max(0,n-own['quantity']));break
        return replace(b,bids=[x for x in bids if x[1]>0],asks=[x for x in asks if x[1]>0])
    def finish_mark(self,key,b,at,rowhash):
        p=self.pending.get(key)
        if p is None or p['stage']=='hold' or at<p['ready']:return
        if b.fresh(at,10000*NS)!='valid' or b.source<p['ready']:
            self.counts['censored_'+p['stage']+'_first_book_clock']+=1;self.pending.pop(key);return
        b=self.adjusted(b)
        values={side:b.value(side,'buy' if p['stage']=='entry' else 'sell',1) for side in ('YES','NO')}
        if any(v is None for v in values.values()):self.counts['censored_depth']+=1;self.pending.pop(key);return
        if p['stage']=='entry':
            p.update(stage='hold',entry=values,entry_at=at,entry_hash=rowhash,target=at+HORIZON*NS);p.pop('deadline');self.counts['entry_marks']+=1
        else:
            need(len(self.examples)<50000,'EXAMPLE_BOUND')
            p['row'].update(yes=(values['YES']-p['entry']['YES']-5000)/UNIT,no=(values['NO']-p['entry']['NO']-5000)/UNIT,
                yesExact=NATIVE.money(values['YES']-p['entry']['YES']-5000),noExact=NATIVE.money(values['NO']-p['entry']['NO']-5000),yesUnits=values['YES']-p['entry']['YES']-5000,noUnits=values['NO']-p['entry']['NO']-5000,
                entryMarkAvailableNs=p['entry_at'],labelAvailableNs=at,available=(at+NS-1)//NS,end=((p['target']+NS-1)//NS)+10000+SLACK,
                entryBookHash=p['entry_hash'],exitBookHash=rowhash,actualHoldToLabelMs=(at-p['entry_at'])/NS)
            self.examples.append(p['row']);self.pending.pop(key);self.counts['labels']+=1
    def event(self,e,rowhash):
        at=NATIVE.instant(e.get('processedAt',e['receivedAt']));need(NATIVE.instant(e['receivedAt'])<=at,'RECEIPT_FUTURE');self.advance(at);kind=e['type'];self.counts['events_'+kind]+=1
        if kind=='gap':
            for ex in e.get('exchangeIds',[e.get('exchangeId')]):self.invalidate(ex,'gap')
            return
        if kind=='actual_order_ack':
            q=e['orderId'];side=e['side'];action=e['action'];price=NATIVE.units(e['price']);need(side in ('YES','NO') and action in ('buy','sell'),'OWN_SCOPE')
            identity=sha(compact({k:e[k] for k in ('orderId','actualOrderId','exchangeId','side','action','quantity','price')}))
            if q in self.own_acks:need(self.own_acks[q]==identity,'OWN_ACK_CONFLICT');return
            need(len(self.own_acks)<50000 and len(self.own)<1024,'OWN_ACK_BOUND');self.own_acks[q]=identity
            self.own[q]={'exchange':e['exchangeId'],'price':price if side=='YES' else UNIT-price,'yesBid':(side=='YES')==(action=='buy'),'quantity':NATIVE.integer(e['quantity'],True)};return
        if kind=='actual_order_terminal':self.own.pop(e['orderId'],None);return
        if kind=='actual_fill':
            # Exact duplicate owned fill observations must not remove depth twice.
            fid=('owned',str(e['actualOrderId']),str(e['fillId']));body={k:e.get(k) for k in ('actualOrderId','fillId','side','action','quantity','price','fee','executedAt')};h=sha(compact(body))
            if fid in self.own_fill_ids:need(self.own_fill_ids[fid]==h,'OWN_FILL_CONFLICT');return
            need(len(self.own_fill_ids)<50000,'OWN_FILL_BOUND');self.own_fill_ids[fid]=h
            if e['orderId'] in self.own:self.own[e['orderId']]['quantity']=max(0,self.own[e['orderId']]['quantity']-e['quantity'])
            return
        if kind not in ('book','trade','coverage'):return
        if kind=='book':
            b=NATIVE.read_book(e,at,10000*NS);key=(e['book']['tournamentId'],b.market,b.exchange)
            if b.status!='valid':self.invalidate(b.exchange,'invalid_book');self.counts['unavailable_book_'+b.status]+=1;return
            self.finish_mark(key,b,at,rowhash)
            need(len(self.history)<1024 or key in self.history,'SCOPE_BOUND')
            hist=self.history.setdefault(key,collections.deque());version=(int(e['book']['sequence']),b.source);old=self.versions.get(key)
            economic=sha(compact({k:e['book'][k] for k in ('marketId','exchangeId','bids','asks')}))
            if old:
                need(version>=old[:2],'BOOK_ROLLBACK')
                if version==old[:2]:need(economic==old[2],'BOOK_VERSION_CONFLICT');self.counts['duplicate_book_version']+=1;return
            self.versions[key]=(*version,economic);self.books[key]=b
            while hist and hist[0]['at']<at-50000*NS:hist.popleft()
            bid,bn=b.bids[0];ask,an=b.asks[0];mid=(bid+ask)/2;micro=(ask*bn+bid*an)/(bn+an)
            anchors=[]
            for lag in (1000,5000,30000):
                target=at-lag*NS;anchor=next((x for x in reversed(hist) if x['at']<=target),None)
                anchors.append(anchor if anchor and target-anchor['at']<=20000*NS else None)
            block=at//(60000*NS)
            if self.blocks.get(key)!=block:
                self.blocks[key]=block;self.counts['first_scope_blocks']+=1
                if key in self.pending:self.counts['block_existing_pending']+=1
                elif any(a is None for a in anchors):
                    self.counts['block_missing_lag_anchors']+=1
                    for lag,a in zip((1,5,30),anchors):
                        if a is None:self.counts['missing_anchor_'+str(lag)+'s']+=1
                else:
                    x=[(micro-mid)/UNIT,(bn-an)/(bn+an),(ask-bid)/UNIT]+[(mid-a['mid'])/UNIT for a in anchors]
                    need(all(math.isfinite(v) for v in x),'FEATURE_FINITE')
                    ready=self.pacer.packet((at+NS-1)//NS,3000)
                    if ready is None:self.counts['block_virtual_budget_unavailable']+=1
                    else:
                        cov=self.coverage.get(key);volume=None
                        if cov and cov['from']<=at-30000*NS and cov['to']>=at and cov['sequence']>=version[0]:
                            volume=sum(q for executed,q in self.trades.get(key,()) if at-30000*NS<=executed<=at)
                        self.counts['volume_available' if volume is not None else 'volume_missing']+=1
                        group=self.groups.get(b.market,b.market)
                        row={'id':sha(compact([rowhash,key,at,PROTOCOL_SHA])),'market':b.market,'scope':key,'group':group,'groupHeldOut':group_holdout(group),'at':(at+NS-1)//NS,'decisionNs':at,'x':x,'unchangedYes':-(ask-bid+5000)/UNIT,'unchangedNo':-(ask-bid+5000)/UNIT,'unsignedVolume30s':volume,'lagAnchorHashes':[a['hash'] for a in anchors]}
                        need(len(self.pending)<1024,'PENDING_BOUND');self.pending[key]={'stage':'entry','row':row,'ready':ready*NS,'deadline':(ready+SLACK)*NS};self.counts['nominations']+=1
            need(len(hist)<2048,'HISTORY_BOUND');hist.append({'at':at,'mid':mid,'hash':rowhash})
        elif kind=='coverage':
            ex=e['exchangeId'];frm=NATIVE.instant(e['from']);to=NATIVE.instant(e['to']);need(e['complete'] is True and frm<=to<=at,'COVERAGE_CLOCK')
            for key in self.history:
                if key[2]==ex:self.coverage[key]={'from':frm,'to':to,'sequence':int(e['projectedThroughSequence'])}
        else:
            key=(e['tournamentId'],e['marketId'],e['exchangeId']);executed=NATIVE.instant(e['executedAt']);received=NATIVE.instant(e['receivedAt']);need(executed<=received<=at,'TRADE_CLOCK')
            ident=('public',*key,str(e['id']));economic=sha(compact({k:e[k] for k in ('tournamentId','marketId','exchangeId','id','yesPrice','quantity','executedAt')}))
            if ident in self.trade_ids:need(self.trade_ids[ident]==economic,'TRADE_CONFLICT');self.counts['duplicate_trade']+=1;return
            self.remember(ident,economic,at)
            if executed<at-30000*NS:self.counts['old_trade']+=1;return
            q=NATIVE.integer(e['quantity'],True)
            need(len(self.trades)<1024 or key in self.trades,'TRADE_SCOPE_BOUND')
            trades=self.trades.setdefault(key,collections.deque());trades.append((executed,q))
            # Execution clocks need not arrive sorted. Each retained observation is
            # visited once per bounded30s window; bound prevents unbounded scans.
            while trades and trades[0][0]<at-30000*NS:trades.popleft()
            need(sum(len(v) for v in self.trades.values())<=32768,'TRADE_BOUND')
    def remember(self,key,economic,at):
        need(len(self.trade_ids)<32768,'ID_BOUND');self.trade_ids[key]=economic;self.trade_fifo.append((at,key))

def fit(rows,indices,side):
    means=[sum(r['x'][j] for r in rows)/len(rows) for j in indices]
    scales=[max(1e-9,math.sqrt(sum((r['x'][j]-m)**2 for r in rows)/len(rows))) for j,m in zip(indices,means)]
    design=[[1.0]+[(r['x'][j]-m)/s for j,m,s in zip(indices,means,scales)] for r in rows];k=len(indices)+1
    a=[[sum(x[i]*x[j] for x in design)+(1.0 if i==j and i else 0.0) for j in range(k)] for i in range(k)]
    b=[sum(x[i]*r[side] for x,r in zip(design,rows)) for i in range(k)]
    return {'indices':indices,'means':means,'scales':scales,'coefficients':MATH.solve(a,b)}
def predict(model,row):return sum(a*b for a,b in zip(model['coefficients'],[1.0]+[(row['x'][j]-m)/s for j,m,s in zip(model['indices'],model['means'],model['scales'])]))
def evaluate(rows,window=None):
    folds=[];scores={k:[] for k in ('unchanged_bbo','sig_local_imbalance_spread','sig_six_features')}
    if rows:
        first,last=window or (min(r['at'] for r in rows),max(r['at'] for r in rows))
        boundaries=[first+(last-first)*i//4 for i in (1,2,3)]
        for i,start in enumerate(boundaries):
            stop=boundaries[i+1] if i<2 else math.inf
            train=[r for r in rows if not r['groupHeldOut'] and r['end']<start and r['available']<=start]
            test=[r for r in rows if start<=r['at']<stop]
            fold={'testStartMs':start,'testEndExclusiveMs':None if math.isinf(stop) else stop,'trainingClusters':len(train),'testClusters':len(test),'trainingGroups':sorted({r['group'] for r in train}),'trainingIdsSha256':sha(compact([r['id'] for r in train])),'trained':len(train)>=16};folds.append(fold)
            if len(train)<16:continue
            models={name:{side:fit(train,cols,side) for side in ('yes','no')} for name,cols in [('sig_local_imbalance_spread',[1,2]),('sig_six_features',list(range(6)))]}
            fold['modelsResearchOnly']=models
            for row in test:
                for name in scores:
                    ps={s:row['unchangedYes' if s=='yes' else 'unchangedNo'] if name=='unchanged_bbo' else predict(models[name][s],row) for s in ('yes','no')}
                    side='yes' if ps['yes']>=ps['no'] else 'no';scores[name].append({'id':row['id'],'market':row['market'],'group':row['group'],'groupHeldOut':row['groupHeldOut'],'prediction':ps[side],'actualUnits':row[side+'Units'],'mse':sum((ps[s]-row[s])**2 for s in ('yes','no'))/2})
    out={}
    for name,scored in scores.items():
        selected=[r for r in scored if r['prediction']>0];groups=collections.defaultdict(list)
        for r in selected:groups[r['group']].append(r['actualUnits'])
        loo=[sum(sum(v) for k,v in groups.items() if k!=g)/sum(len(v) for k,v in groups.items() if k!=g) for g in groups] if len(groups)>1 else []
        out[name]={'leaveOneSelectedGroupOutMeanUnitsRange':[min(loo),max(loo)] if loo else None,'largestSelectedGroupFraction':max(map(len,groups.values()))/len(selected) if selected else None,'statisticalIndependenceClaimed':False,'oosClusters':len(scored),'selectedClusters':len(selected),'selectedNetCrossingUnits':sum(r['actualUnits'] for r in selected),'mse':sum(r['mse'] for r in scored)/len(scored) if scored else None,'selectedMarkets':len({r['market'] for r in selected}),'selectedGroups':len(groups),'equalGroupMeanUnits':sum(sum(v)/len(v) for v in groups.values())/len(groups) if groups else None,'heldOutGroupOosClusters':sum(r['groupHeldOut'] for r in scored),'heldOutGroupSelectedClusters':sum(r['groupHeldOut'] for r in selected),'heldOutGroupSelectedNetUnits':sum(r['actualUnits'] for r in selected if r['groupHeldOut']),'sampleIdsSha256':sha(compact([r['id'] for r in scored])),'selectionIdsSha256':sha(compact([r['id'] for r in selected])),'minimumCountPass':len(scored)>=100 and len(selected)>=100}
    return {'status':'UNQUALIFIED_RESEARCH','folds':folds,'models':out,'executionAuthority':False,'modelApproval':False,'profitabilityEstablished':False}

def run(manifest,expected):
    need(sha((HERE/'protocol.json').read_bytes().replace(b'\r\n',b'\n'))==PROTOCOL_SHA,'PROTOCOL_CHANGED')
    path=Path(manifest).absolute();need(path.stat().st_size<=1024*1024 and sha(path.read_bytes())==expected,'MANIFEST_PIN')
    value=json.loads(path.read_bytes(),object_pairs_hook=MATH.unique_object);need(value['schemaVersion']==1 and value.get('sealed') is True,'MANIFEST_SCHEMA')
    required={'journal':2*1024**3,'anchorHead':64*1024**2,'report':64*1024**2,'bankManifest':64*1024**2};pins={k:Pin(path.parent,value[k],bound) for k,bound in required.items()}
    for optional in ('finalHead','boundary','anchorBoundary','semanticGroups'):
        if optional in value:pins[optional]=Pin(path.parent,value[optional],64*1024**2)
    anchor=NATIVE.native_sealed_object(pins['anchorHead'].path.read_bytes());report=NATIVE.native_sealed_object(pins['report'].path.read_bytes());bank=NATIVE.native_sealed_object(pins['bankManifest'].path.read_bytes())
    need(anchor['manifestHash']==report['manifestHash']==bank['hash'],'BANK_IDENTITY')
    if 'finalHead' in pins:
        final=NATIVE.native_sealed_object(pins['finalHead'].path.read_bytes());need(final['sequence']==report['sequence'] and final['previous']==report['previous'] and final['manifestHash']==report['manifestHash'],'FINAL_HEAD')
    groups=pins['semanticGroups'].json() if 'semanticGroups' in pins else {};need(isinstance(groups,dict) and len(groups)<=1024 and all(isinstance(k,str) and isinstance(v,str) and v for k,v in groups.items()),'GROUP_MAP')
    screen=Screen(groups);previous=anchor['previous'];sequence=anchor['sequence'];cutoff=NATIVE.instant(report['report']['at']);count=0
    with pins['journal'].path.open('rb') as f:
        while True:
            raw=f.readline(NATIVE.MAX_ROW_BYTES+1)
            if not raw:break
            need(len(raw)<=NATIVE.MAX_ROW_BYTES and count<1000000,'ROW_BOUND');row=NATIVE.native_row(raw);need(row['sequence']==sequence+1 and row['previous']==previous,'SUFFIX_CHAIN')
            at=NATIVE.instant(row['event'].get('processedAt',row['event']['receivedAt']));need(at<=cutoff,'AFTER_CUTOFF');screen.event(row['event'],row['hash']);previous=row['hash'];sequence=row['sequence'];count+=1
    need(sequence==report['sequence'] and previous==report['previous'] and screen.last==cutoff,'FINAL_PREFIX_MISMATCH')
    for pin in pins.values():pin.finish()
    need(sha(path.read_bytes())==expected,'MANIFEST_CHANGED')
    result={'protocolSha256':PROTOCOL_SHA,'sourceCodeSha256':sha(Path(__file__).read_bytes()),'inputManifestSha256':expected,'inputPins':{k:v.public() for k,v in pins.items()},'nativeSuffix':{'firstSequence':anchor['sequence']+1,'lastSequence':sequence,'rows':count,'exactTailMatched':True},'counts':dict(screen.counts),'pendingAtCutoff':len(screen.pending),'pendingByStage':dict(collections.Counter(x['stage'] for x in screen.pending.values())),'completedCommonCohort':len(screen.examples),'completedMarkets':len({r['market'] for r in screen.examples}),'commonCohortNonzeroFeatureCounts':{name:sum(r['x'][i]!=0 for r in screen.examples) for i,name in enumerate(FEATURES)},'semanticGroupsAvailable':bool(groups),'groupMapCoverageMarkets':sum(m in groups for m in {r['market'] for r in screen.examples}),'cohortSha256':sha(compact([r['id'] for r in screen.examples])),'evaluation':evaluate(screen.examples,((screen.first+NS-1)//NS,cutoff//NS)),'sourceBodyMembershipIndependentlyVerified':False,'initialOwnOrderSnapshotAvailable':False,'actualSdkBudgetVerified':False,'executionAuthority':False,'modelApproval':False,'profitabilityEstablished':False,'limitations':['Original native suffix membership/hash linkage is not financial Session replay or independent raw-source-body authentication.','Virtual pacing assumes no competing account load and zero HTTP duration; these are displayed one-share counterfactuals, never fills.','Pre-anchor own resting order state unavailable; only suffix-known owned depth removed.','Missing observations/coverage are selective; market/time blocks and held-out markets do not remove common election/factor correlation.']}
    return result

def main():
    a=argparse.ArgumentParser();a.add_argument('--manifest',required=True);a.add_argument('--manifest-sha256',required=True);a.add_argument('--output',required=True);args=a.parse_args()
    output=Path(args.output).absolute();need(not output.exists(),'OUTPUT_EXISTS');result=run(args.manifest,args.manifest_sha256)
    with output.open('x',encoding='utf-8',newline='\n') as f:json.dump(result,f,indent=2,allow_nan=False);f.write('\n')
if __name__=='__main__':main()
