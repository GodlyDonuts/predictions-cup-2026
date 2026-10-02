from collect_public import *
for ticker in ['HOUSEVA1-26-D','HOUSENH1-26-R','SENATEAK-26-D','SENATEFLS-26-D','SENATENC-26-D','GOVPARTYMI-26-D']:
 d,r=get('markets/'+ticker+'/orderbook',{},'orderbook_'+ticker)
 print('book keys',list(d),'sample',str(d)[:400],flush=True)
