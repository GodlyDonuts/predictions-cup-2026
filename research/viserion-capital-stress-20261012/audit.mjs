import fs from 'node:fs';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {stressCapitalScreen} from './screen.mjs';
const root=new URL('../../',import.meta.url), directory=new URL('./',import.meta.url);
const read=path=>{const bytes=fs.readFileSync(new URL(path,root));return {path,sha256:crypto.createHash('sha256').update(bytes).digest('hex'),json:JSON.parse(bytes)};};
const bank=read('data/maine-passive-harvest-paper-20261012-bank.json'),state=read('data/maine-passive-harvest-paper-20261012-state.json');
if(bank.json.bankHash!==state.json.bankHash)throw Error('BANK_STATE_MISMATCH');
const b=bank.json,instrument=b.instruments[0],pending=Object.values(state.json.pending).filter(x=>x.marketId===instrument.marketId&&x.action==='buy');
if(pending.length!==1)throw Error('EXPECTED_ONE_HISTORICAL_BUY_QUOTE');
const quote=pending[0];
// Conservatively floor the recorded numeric sensitivity to micro precision.
const adverse=(Math.floor(b.ordinaryAdverseValue*1e6)/1e6).toFixed(6);
const auditAt=new Date().toISOString();
const result=stressCapitalScreen({auditQuantity:quote.quantity,scenarios:[{id:'declared_14_day_reference',value:instrument.lowerValue},{id:'declared_minus_3_point_margin',value:adverse}],
 sizing:{equityLower:b.initialCash,cashAvailable:state.json.cash,protectedPrice:quote.limit,currentQuantity:0,reservedBuyQuantity:0,depthQuantity:quote.quantity,kellyFractionBps:b.kellyFractionBps,eventFractionBps:b.eventFractionBps,eventPrincipal:'0.000000',grossRemaining:b.capitalLimit},
 evidence:{auditAt,bookAt:state.json.books[instrument.marketId].receivedAt,maxBookAgeMs:b.maxBookAgeMs,sourceCensusComplete:b.sourceCensusComplete,populationTransportValidated:false,feeScheduleVerified:false,independentPerformanceValidated:false}});
const report={...result,auditAt,sources:[{path:bank.path,sha256:bank.sha256},{path:state.path,sha256:state.sha256}],bankHash:b.bankHash,
 historicalQuote:{marketId:instrument.marketId,exchangeId:instrument.exchangeId,eventId:instrument.eventId,side:instrument.side,at:quote.at,expiresAt:quote.expiresAt,price:quote.limit,quantity:quote.quantity},
 currentAccountVerified:false,currentDepthVerified:false,adverseInputRounding:'floored to six decimal places; scenario assumption, not statistical bound',
 reportedPaperFills:state.json.fills.length,reportedPaperRealized:state.json.realized,
 interpretation:'Reference settlement value is not a forecast of short-term harvest exits. This historical shadow audit does not prove a live order, fill, harvest loss, market profit, or a calibrated probability bound.'};
fs.writeFileSync(new URL('results.json',directory),JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({output:fileURLToPath(new URL('results.json',directory)),...result},null,2));
