"""Atlas empirical-Bayes joint poll-error predictive model (NumPy only).

Outcomes train the error distribution, never enter a current-race feature.
Variance-component moments are constrained/shrunk, not a full Bayesian sampler.
All predictions remain research until independently reviewed calibration/transport.
"""
from __future__ import annotations
import hashlib, json, math
from collections import defaultdict
import numpy as np

DAY=86400000
REGIONS={s:r for r,states in {
 'northeast':'CT ME MA NH RI VT NJ NY PA',
 'midwest':'IN IL MI OH WI IA KS MN MO NE ND SD',
 'south':'DE DC FL GA MD NC SC VA WV AL KY MS TN AR LA OK TX',
 'west':'AZ CO ID MT NV NM UT WY AK CA HI OR WA'}.items() for s in states.split()}
CONFIG={'version':1,'recencyHalfLifeDays':21,'housePriorFloorVariance':0.25,
 'statePriorRaces':8,'officePriorRaces':12,'componentFloorVariance':0.25,
 'raceFloorVariance':1.0,'studentDegreesFreedom':4,'minimumTrainingRaces':30,
 'minimumTrainingCycles':3,'driftRidge':10000.0,'maximumRaces':12}
def canonical(x):return json.dumps(x,sort_keys=True,separators=(',',':'),ensure_ascii=False,allow_nan=False).encode()
def sha(x):return hashlib.sha256(canonical(x)).hexdigest()
def groups(rows,key):
 out=defaultdict(list)
 for r in rows:out[r[key]].append(r)
 return dict(out)
def normalize(rows):
 out=[];seen=set()
 for p in rows:
  p=dict(p)
  if p['id'] in seen:raise ValueError('duplicate poll observation')
  seen.add(p['id'])
  if p['state'] not in REGIONS or p['office'] not in ('senate','governor'):raise ValueError('scope unsupported')
  vals=[p['pollMargin'],p['democratic'],p['republican'],p['horizonDays']]
  if not all(math.isfinite(float(v)) for v in vals) or not 0<p['democratic']<100 or not 0<p['republican']<100 or p['democratic']+p['republican']>100.001:raise ValueError('poll values')
  if not p['fieldAtMs']<p['electionAtMs'] or abs((p['electionAtMs']-p['fieldAtMs'])/DAY-p['horizonDays'])>1:raise ValueError('field/election clock')
  p['region']=REGIONS[p['state']];out.append(p)
 for race,ps in groups(out,'raceId').items():
  if len({p['pollsterId'] for p in ps})!=len(ps):raise ValueError('must select one newest independent pollster per race')
  if len({(p['year'],p['office'],p['state'],p['electionAtMs']) for p in ps})!=1:raise ValueError('conflicting race scope')
  if 'actualMargin' in ps[0] and len({p['actualMargin'] for p in ps})!=1:raise ValueError('conflicting outcome')
 return out

def measurement(p,fallback):
 n=p.get('sampleSize')
 if not n or not isinstance(n,(float,int)) or n<=0:return fallback
 d=p['democratic']/100;r=p['republican']/100
 return max(0.01,10000*(d+r-(d-r)**2)/n)
def weights(ps):
 h=np.array([p['horizonDays'] for p in ps]);w=2**(-(h-h.min())/CONFIG['recencyHalfLifeDays']);return w/w.sum()
def race_averages(rows,house=None,drift=0,reference=None,noise=0,fallback=16):
 out=[];house=house or {};reference=reference or {}
 for rid,ps in sorted(groups(rows,'raceId').items()):
  w=weights(ps);p=ps[0];h=float(w@np.array([x['horizonDays'] for x in ps]));office=p['office']
  margin=float(w@np.array([x['pollMargin']+house.get(x['pollsterId'],0)+drift*(x['horizonDays']-reference.get(office,h)) for x in ps]))
  out.append(dict(raceId=rid,year=p['year'],state=p['state'],region=p['region'],office=office,polls=ps,weights=w,margin=margin,horizon=h,
   actual=p.get('actualMargin'),measurement=float(sum(float(a*a)*(measurement(x,fallback)+noise) for a,x in zip(w,ps)))))
 return out

def fit(rows):
 rows=normalize(rows);races=race_averages(rows)
 if len(races)<CONFIG['minimumTrainingRaces'] or len({r['year'] for r in races})<CONFIG['minimumTrainingCycles']:raise ValueError('insufficient training races/cycles')
 # Within-race centering removes shared outcome/error before house/drift fit.
 within=[];num=den=0.0
 for r in races:
  ps=r['polls'];w=r['weights'];errors=np.array([p['actualMargin']-p['pollMargin'] for p in ps]);center=float(w@errors)
  for p,a,y in zip(ps,w,errors):
   dx=p['horizonDays']-r['horizon'];dy=float(y-center);num+=a*dx*dy;den+=a*dx*dx;within.append((p,float(a),dy,dx))
 drift=num/(den+CONFIG['driftRidge']);house_rows=defaultdict(list)
 within_var=sum(w*(dy-drift*dx)**2 for _,w,dy,dx in within)/len(races)
 fallback=float(np.median([measurement(p,16) for p in rows if p.get('sampleSize')])) if any(p.get('sampleSize') for p in rows) else 16.0
 noise=max(0.0,within_var-sum(w*measurement(p,fallback) for p,w,_,_ in within)/len(races))
 for p,w,dy,dx in within:house_rows[p['pollsterId']].append((w,dy-drift*dx))
 raw={k:sum(w*y for w,y in v)/sum(w for w,_ in v) for k,v in house_rows.items()}
 house_var=max(CONFIG['housePriorFloorVariance'],float(np.mean([x*x for x in raw.values()]))-max(1.0,within_var)/max(1.0,float(np.mean([sum(w for w,_ in v) for v in house_rows.values()]))))
 house={};house_unc={}
 for k,v in house_rows.items():
  precision=sum(w for w,_ in v)/max(1.0,within_var);post=1/(1/house_var+precision);house[k]=post*precision*raw[k];house_unc[k]=post
 reference={o:float(np.mean([r['horizon'] for r in races if r['office']==o])) for o in ('senate','governor')}
 races=race_averages(rows,house,drift,reference,noise,fallback)
 error=np.array([r['actual']-r['margin'] for r in races]);global_bias=float(error.mean());bias={}
 for office in reference:
  es=[float(e) for r,e in zip(races,error) if r['office']==office];n=len(es)
  bias[office]=(sum(es)+CONFIG['officePriorRaces']*global_bias)/(n+CONFIG['officePriorRaces'])
 state_errors=defaultdict(list)
 for r,e in zip(races,error):state_errors[r['state']].append((r['year'],float(e)-bias[r['office']]))
 state_bias={s:sum(e for _,e in es)/(len(es)+CONFIG['statePriorRaces']) for s,es in state_errors.items()}
 # Cross-cycle state means for estimating new-cycle error, not own-cycle fitted residuals.
 residual=[]
 for r,e in zip(races,error):
  past=[v for y,v in state_errors[r['state']] if y!=r['year']]
  correction=sum(past)/(len(past)+CONFIG['statePriorRaces'])
  residual.append(float(e)-bias[r['office']]-correction)
 residual=np.array(residual);cycle_indices=groups([dict(r,index=i) for i,r in enumerate(races)],'year')
 def pair_cov(predicate):
  values=[];pairs=0
  for rs in cycle_indices.values():
   xs=[residual[a['index']]*residual[b['index']] for i,a in enumerate(rs) for b in rs[i+1:] if predicate(a,b)]
   if xs:values.append(float(np.mean(xs)));pairs+=len(xs)
  return (float(np.mean(values)) if values else 0.0,len(values),pairs)
 n,nc,npairs=pair_cov(lambda a,b:a['region']!=b['region'])
 reg,rc,rpairs=pair_cov(lambda a,b:a['region']==b['region'] and a['state']!=b['state'])
 st,sc,spairs=pair_cov(lambda a,b:a['state']==b['state'] and a['raceId']!=b['raceId'])
 floor=CONFIG['componentFloorVariance'];national=max(floor,n);regional=max(floor,reg-national);state=max(floor,st-national-regional)
 total=max(4.0,float(np.mean(residual**2))-float(np.mean([r['measurement'] for r in races])))
 shared=national+regional+state
 if shared>max(0.75,total-CONFIG['raceFloorVariance']):
  scale=max(0.75,total-CONFIG['raceFloorVariance'])/shared;national*=scale;regional*=scale;state*=scale
 race=max(CONFIG['raceFloorVariance'],total-national-regional-state)
 state_unc={s:total/(len(es)+CONFIG['statePriorRaces']) for s,es in state_errors.items()}
 cycles=sorted(cycle_indices)
 return {'schemaVersion':1,'algorithm':'atlas_eb_joint_t4_v1','config':dict(CONFIG),'bias':bias,'stateBias':state_bias,'stateUncertainty':state_unc,'house':house,'houseUncertainty':house_unc,'houseVariance':house_var,'measurementOverdispersion':noise,'missingSampleVariance':fallback,'referenceHorizon':reference,'trainingHorizonRange':{o:[min(p['horizonDays'] for p in rows if p['office']==o),max(p['horizonDays'] for p in rows if p['office']==o)] for o in reference},'driftPerDay':drift,'driftVariance':max(1.0,within_var)/(den+CONFIG['driftRidge']),
  'variance':{'national':national,'regional':regional,'state':state,'race':race},'varianceEvidence':{'nationalCycles':nc,'nationalPairs':npairs,'regionalCycles':rc,'regionalPairs':rpairs,'stateCycles':sc,'statePairs':spairs},'races':len(races),'pollsterRaceObservations':len(rows),'cycles':cycles,'trainedThroughYear':max(cycles),'trainingCutoffMs':max(p['electionAtMs'] for p in rows),'officeMeanVariance':{o:total/max(3,len({r['year'] for r in races if r['office']==o})) for o in reference}}

def posterior(model,rows):
 rows=normalize(rows);rs=race_averages(rows,model['house'],model['driftPerDay'],model['referenceHorizon'],model['measurementOverdispersion'],model['missingSampleVariance'])
 if not rs or len(rs)>64:raise ValueError('posterior race bound')
 means=np.array([r['margin']+model['bias'][r['office']]+model['stateBias'].get(r['state'],0) for r in rs]);factors={}
 def load(key,i,x):
  if key not in factors:factors[key]=np.zeros(len(rs))
  factors[key][i]+=x
 for i,r in enumerate(rs):
  scale=math.sqrt(max(.5,min(2.0,r['horizon']/model['referenceHorizon'][r['office']])))
  for name,key in [('national','national'),('regional','region:'+r['region']),('state','state:'+r['state']),('race','race:'+r['raceId'])]:load(key,i,math.sqrt(model['variance'][name])*scale)
  load('state-mean:'+r['state'],i,math.sqrt(model['stateUncertainty'].get(r['state'],model['variance']['race']/8)))
  load('office-mean:'+r['office'],i,math.sqrt(model['officeMeanVariance'][r['office']]))
  load('drift-mean',i,(r['horizon']-model['referenceHorizon'][r['office']])*math.sqrt(model['driftVariance']))
  load('measurement:'+r['raceId'],i,math.sqrt(r['measurement']))
  for p,w in zip(r['polls'],r['weights']):load('house-mean:'+p['pollsterId'],i,float(w)*math.sqrt(model['houseUncertainty'].get(p['pollsterId'],model['houseVariance'])))
 matrix=np.stack(list(factors.values()),axis=1);cov=matrix@matrix.T
 return rs,means,cov

def t4cdf(x):
 y=x/math.sqrt(x*x+4);return .5+.75*y-.25*y*y*y

def probabilities(means,cov):return np.array([t4cdf(float(m)/math.sqrt(float(v)/2)) for m,v in zip(means,np.diag(cov))])
def draw_joint(means,cov,draws=65536,seed=1701):
 if not 256<=draws<=262144:raise ValueError('draw bound')
 rng=np.random.default_rng(seed);l=np.linalg.cholesky(cov+np.eye(len(means))*1e-10)
 z=rng.normal(size=(draws,len(means)))@l.T
 # One common scale preserves the learned joint t-copula, not independent errors.
 return means+z*np.sqrt(2/rng.chisquare(4,size=(draws,1)))

def metrics(rows):
 if not rows:return None
 p=np.clip([r['probability'] for r in rows],1e-9,1-1e-9);y=np.array([r['won'] for r in rows],float)
 return {'races':len(rows),'cycles':len({r['year'] for r in rows}),'brier':float(np.mean((p-y)**2)),'logLoss':float(-np.mean(y*np.log(p)+(1-y)*np.log(1-p))),'correct':int(np.sum((p>.5)==y)),'marginRmse':float(np.sqrt(np.mean([(r['actual']-r['mean'])**2 for r in rows]))),'interval90Coverage':float(np.mean([abs(r['actual']-r['mean'])<=2.13184678632665*r['sd']/math.sqrt(2) for r in rows])),
  'bins':[{'lower':a/5,'upper':(a+1)/5,'n':int(sum(a/5<=x<((a+1)/5 if a<4 else 1.000001) for x in p)),'mean':float(np.mean([x for x in p if a/5<=x<((a+1)/5 if a<4 else 1.000001)])) if any(a/5<=x<((a+1)/5 if a<4 else 1.000001) for x in p) else None,'observed':float(np.mean([r['won'] for r,x in zip(rows,p) if a/5<=x<((a+1)/5 if a<4 else 1.000001)])) if any(a/5<=x<((a+1)/5 if a<4 else 1.000001) for x in p) else None} for a in range(5)]}

def chronological_joint_pairs(races,means,cov,ps,year):
 # Exactly the existing LOCO sampler, draw count, year seed and pair target.
 draws=draw_joint(means,cov,4096,year);win=(draws>0).astype(float);joint=win.T@win/len(win)
 return [{'year':year,'raceIds':[races[j]['raceId'],races[i]['raceId']],
          'jointProbability':float(joint[i,j]),'independentMarginalsProbability':float(ps[i]*ps[j]),
          'bothDemocraticWon':bool(races[i]['actual']>0 and races[j]['actual']>0)}
         for i in range(len(races)) for j in range(i)]

def chronological_joint_uncertainty(folds):
 # One paired mean per election cycle, never thousands of IID race pairs.
 scored=[f for f in folds if f['pairs']>0]
 if not scored:return {'electionCycleBlocks':0,'pairs':0,'equalCycleJointPairBrier':None,'equalCycleIndependentMarginalsPairBrier':None,'meanJointMinusIndependent':None,'percentile95Interval':None,'bootstrapReplicates':2000,'notAConfidenceGuarantee':True}
 if len({f['heldOutYear'] for f in scored})!=len(scored):raise ValueError('duplicate joint election-cycle block')
 deltas=np.array([f['jointPairBrier']-f['independentMarginalsPairBrier'] for f in scored])
 rng=np.random.default_rng(1701);sample=rng.choice(deltas,size=(2000,len(scored)),replace=True).mean(axis=1)
 return {'electionCycleBlocks':len(scored),'pairs':sum(f['pairs'] for f in scored),
         'equalCycleJointPairBrier':float(np.mean([f['jointPairBrier'] for f in scored])),
         'equalCycleIndependentMarginalsPairBrier':float(np.mean([f['independentMarginalsPairBrier'] for f in scored])),
         'meanJointMinusIndependent':float(deltas.mean()),'percentile95Interval':list(map(float,np.quantile(sample,[.025,.975]))),
         'bootstrapReplicates':2000,'bootstrapSeed':1701,'aggregation':'equal election-cycle means; within-cycle pairs remain together',
         'statistic':'earlier-years-only joint minus product-marginals pair Brier; lower is better','notAConfidenceGuarantee':True}

def validate(rows):
 rows=normalize(rows);years=sorted({p['year'] for p in rows});predictions=[];chronological=[];pair_scores=[];folds=[];chronological_pairs=[];chronological_folds=[]
 for year in years:
  train=[p for p in rows if p['year']!=year];test=[p for p in rows if p['year']==year];model=fit(train);rs,means,cov=posterior(model,test);ps=probabilities(means,cov)
  assert year not in model['cycles']
  raw_test={r['raceId']:r['margin'] for r in race_averages(test)};baseline_races=race_averages(train);errors=[r['actual']-r['margin'] for r in baseline_races];baseline_bias=float(np.mean(errors));baseline_sd=max(3,float(np.std(errors)))
  for r,m,v,p in zip(rs,means,np.diag(cov),ps):predictions.append({'raceId':r['raceId'],'year':year,'won':r['actual']>0,'actual':r['actual'],'mean':float(m),'sd':math.sqrt(float(v)),'probability':float(p),'baselineMean':raw_test[r['raceId']]+baseline_bias,'baselineSd':baseline_sd,'baselineProbability':t4cdf((raw_test[r['raceId']]+baseline_bias)/(baseline_sd/math.sqrt(2)))})
  draws=draw_joint(means,cov,4096,year);win=(draws>0).astype(float);joint=win.T@win/len(win)
  scores=[];baseline_scores=[]
  for i in range(len(rs)):
   for j in range(i):
    target=float(rs[i]['actual']>0 and rs[j]['actual']>0);scores.append((joint[i,j]-target)**2);baseline_scores.append((ps[i]*ps[j]-target)**2)
  if scores:pair_scores.append({'year':year,'pairs':len(scores),'jointPairBrier':float(np.mean(scores)),'independentMarginalsPairBrier':float(np.mean(baseline_scores))})
  folds.append({'heldOutYear':year,'trainingYears':model['cycles'],'trainingRaces':model['races'],'testRaces':len(rs)})
  past=[p for p in rows if p['year']<year]
  if len({p['raceId'] for p in past})>=30 and len({p['year'] for p in past})>=3:
   m=fit(past);rr,mm,cc=posterior(m,test);pp=probabilities(mm,cc);assert m['trainedThroughYear']<year
   for r,mu,v,p in zip(rr,mm,np.diag(cc),pp):chronological.append({'raceId':r['raceId'],'year':year,'won':r['actual']>0,'actual':r['actual'],'mean':float(mu),'sd':math.sqrt(float(v)),'probability':float(p),'trainedThroughYear':m['trainedThroughYear']})
   pairs=chronological_joint_pairs(rr,mm,cc,pp,year);chronological_pairs.extend(pairs)
   chronological_folds.append({'heldOutYear':year,'trainingYears':m['cycles'],'trainedThroughYear':m['trainedThroughYear'],
    'trainingCutoffMs':m['trainingCutoffMs'],'trainingRaces':m['races'],'testRaceIds':[r['raceId'] for r in rr],
    'testRaces':len(rr),'pairs':len(pairs),'draws':4096,'seed':year,
    'jointPairBrier':float(np.mean([(p['jointProbability']-p['bothDemocraticWon'])**2 for p in pairs])) if pairs else None,
    'independentMarginalsPairBrier':float(np.mean([(p['independentMarginalsProbability']-p['bothDemocraticWon'])**2 for p in pairs])) if pairs else None})
 baseline=[dict(r,probability=r['baselineProbability'],mean=r['baselineMean'],sd=r['baselineSd']) for r in predictions]
 # Election-cycle block bootstrap of paired Brier differences; no IID-race CI.
 rng=np.random.default_rng(1701);blocks={y:[r for r in predictions if r['year']==y] for y in years};deltas=[]
 for _ in range(2000):
  sample=[r for y in rng.choice(years,len(years),replace=True) for r in blocks[int(y)]]
  deltas.append(float(np.mean([(r['probability']-r['won'])**2-(r['baselineProbability']-r['won'])**2 for r in sample])))
 uncertainty={'statistic':'Atlas minus pooled baseline paired Brier; lower is better','electionCycleBlocks':len(years),'bootstrapReplicates':2000,'percentile95Interval':list(map(float,np.quantile(deltas,[.025,.975]))),'notAConfidenceGuarantee':True}
 return {'chronologicalJointFolds':chronological_folds,'chronologicalJointPairRows':chronological_pairs,'chronologicalJointCycleBootstrap':chronological_joint_uncertainty(chronological_folds),'pairedBrierCycleBootstrap':uncertainty,'leaveElectionCycleOut':metrics(predictions),'sameFoldPooledErrorBaseline':metrics(baseline),'chronologicalOutcomeDiagnostic':metrics(chronological),'pairwiseJointDiagnostic':pair_scores,'folds':folds,'predictionRows':predictions,'chronologicalRows':chronological,'limitations':['LOCO is retrospective: other later cycles enter training; chronological diagnostic trains earlier years only.','Original historical release clocks/populations are absent; field dates cannot prove then-available forecasts.','Final-candidate identity selection and revised outcome archives prevent untouched historical-vintage claims.','Variance-component EB moments/floors, state/house shrinkage and t4 tail are declared modeling assumptions, not a fully sampled hyperparameter posterior.','Poll error already includes undecided resolution/drift; no additional adverse undecided allocation is silently added.','No executable historical market prices, fill costs or trading PnL.']}

def integer_tree(value):
 if isinstance(value,float):return int(round(value*1000000))
 if isinstance(value,dict):return {k:integer_tree(v) for k,v in value.items()}
 if isinstance(value,(tuple,list)):return [integer_tree(v) for v in value]
 return value

def artifact(model,current,provenance,calibration,observed_at,expires_at,draws=65536,seed=1701):
 if not observed_at<expires_at:raise ValueError('artifact lifetime')
 if any(p.get('availableAtMs') is None or p['availableAtMs']>observed_at or p['fieldAtMs']>observed_at for p in current):raise ValueError('current source not yet demonstrably observed')
 current=normalize(current)
 if len({p['raceId'] for p in current})>CONFIG['maximumRaces']:raise ValueError('compact artifact supports <=12 races')
 if any(p['year']<=model['trainedThroughYear'] for p in current):raise ValueError('artifact forecast is not after training cutoff')
 rs,means,cov=posterior(model,current);samples=draw_joint(means,cov,draws,seed)>0;counts=defaultdict(int)
 for row in samples:counts[''.join('D' if x else 'R' for x in row)]+=1
 keys=sorted(counts);raw=[counts[k]*1000000 for k in keys];weights=[n//draws for n in raw]
 for i in sorted(range(len(keys)),key=lambda i:(-(raw[i]%draws),keys[i]))[:1000000-sum(weights)]:weights[i]+=1
 scenarios=[{'id':'atlas:'+k,'weightPpm':w,'raceOutcomes':['DEM' if x=='D' else 'REP' for x in k]} for k,w in zip(keys,weights) if w]
 assert sum(s['weightPpm'] for s in scenarios)==1000000
 body={'schemaVersion':1,'objective':'ordinary_terminal_race_outcome','observedAtMs':observed_at,'expiresAtMs':expires_at,'trainingCutoffMs':model['trainingCutoffMs'],
  'qualification':{'kind':'research','historicalAvailabilityClocksKnown':False,'historicalPopulationKnown':False,'allCurrentPollHorizonsWithinTrainingSupport':all(model['trainingHorizonRange'][p['office']][0]<=p['horizonDays']<=model['trainingHorizonRange'][p['office']][1] for p in current),'ordinaryOnly':True,'exceptionalStateProbabilitiesIdentified':False,'currentSourceOriginalsVerified':False,'varianceHyperparametersFullyBayesian':False,'executionAuthority':False},
  'races':[{'raceId':r['raceId'],'state':r['state'],'office':r['office']} for r in rs],'scenarios':scenarios,
  'sources':provenance['sources'],'calibrationContinuousValueScale':1000000,'calibration':integer_tree({k:v for k,v in calibration.items() if k not in ('predictionRows','chronologicalRows','chronologicalJointPairRows')}),
  'declaredModel':{'algorithm':model['algorithm'],'parametersScaledBy':1000000,'fittedParameterSha256':sha(model),'configMicrounits':integer_tree(model['config']),'varianceMicropointsSquared':integer_tree(model['variance']),'varianceEvidence':model['varianceEvidence'],'trainingRaces':model['races'],'trainingPollsterRaceObservations':model['pollsterRaceObservations'],'sampling':{'draws':draws,'seed':seed,'jointTail':'shared_t4_scale'},'marginMeanMicropoints':[int(round(x*1000000)) for x in means],'marginCovarianceMicropointsSquared':[[int(round(x*1000000000000)) for x in row] for row in cov]},
  'currentInputSha256':sha(current),'limitations':calibration['limitations']+['Current retained poll source originals are not independently reloaded by the sample builder; research artifact only.','Ordinary DEM/REP outcomes are not contractual settlement/party/refund certificates.','A terminal posterior is not a deadline liquidation cash estimate.']}
 body['modelSha256']=sha(body);return body
