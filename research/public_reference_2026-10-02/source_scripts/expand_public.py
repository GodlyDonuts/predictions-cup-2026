from collect_public import *
series=json.loads((ROOT/'eligible_series_catalog.json').read_text())
d=json.loads((ROOT/'raw/007_kxhouserace_events_1.json').read_text()); cursor=d.get('cursor');page=1
while cursor:
 page+=1;d,r=get('events',{'series_ticker':'KXHOUSERACE','with_nested_markets':'true','limit':200,'cursor':cursor},f'kxhouserace_events_{page}')
 print('house page',page,'events',len(d.get('events',[])),'cursor',d.get('cursor'),flush=True);cursor=d.get('cursor')
for s in series:
 if s['ticker'].startswith('KX') and s['ticker']!='KXHOUSERACE':
  d,r=get('events',{'series_ticker':s['ticker'],'with_nested_markets':'true','limit':200},'series_events_'+s['ticker'])
  print('events',[(e['event_ticker'],e['sub_title'],len(e.get('markets',[]))) for e in d.get('events',[])],'cursor',d.get('cursor'),flush=True)
  if d.get('cursor'):raise RuntimeError('Unexpected extra series pagination: review budget before continuing')
