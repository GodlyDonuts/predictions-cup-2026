import fs from 'node:fs';
import readline from 'node:readline';
import path from 'node:path';
import { parseCsv, senateRaces, presidentialTables, predict, probabilityFromMargin } from './model.mjs';
const base='research/election-baseline-v2', v1='research/election-baseline-v1', report=JSON.parse(fs.readFileSync(`${base}/model-summary-affiliation-invariant.json`));
const senate=senateRaces(parseCsv(fs.readFileSync(`${v1}/sources/1976-2024-senate-state.csv`,'utf8')),JSON.parse(fs.readFileSync(`${base}/final-stage-overrides.json`)));
const president=presidentialTables(parseCsv(fs.readFileSync(`${v1}/sources/1976-2024-president.csv`,'utf8')));
const stateNames=['Alabama:AL','Alaska:AK','Arizona:AZ','Arkansas:AR','California:CA','Colorado:CO','Connecticut:CT','Delaware:DE','Florida:FL','Georgia:GA','Hawaii:HI','Idaho:ID','Illinois:IL','Indiana:IN','Iowa:IA','Kansas:KS','Kentucky:KY','Louisiana:LA','Maine:ME','Maryland:MD','Massachusetts:MA','Michigan:MI','Minnesota:MN','Mississippi:MS','Missouri:MO','Montana:MT','Nebraska:NE','Nevada:NV','New Hampshire:NH','New Jersey:NJ','New Mexico:NM','New York:NY','North Carolina:NC','North Dakota:ND','Ohio:OH','Oklahoma:OK','Oregon:OR','Pennsylvania:PA','Rhode Island:RI','South Carolina:SC','South Dakota:SD','Tennessee:TN','Texas:TX','Utah:UT','Vermont:VT','Virginia:VA','Washington:WA','West Virginia:WV','Wisconsin:WI','Wyoming:WY'];
const catalogue=JSON.parse(fs.readFileSync('data/cup-identity-readonly-20261004.json')).markets;
const runtime=process.env.BAAL_RUNTIME || path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'), bookFile=path.join(runtime,'public-books.jsonl');
const byteCutoff=fs.statSync(bookFile).size, latest=new Map();
// Freeze the byte boundary of the append-only public tape; no API traffic.
for await(const line of readline.createInterface({input:fs.createReadStream(bookFile,{end:byteCutoff-1}),crlfDelay:Infinity})){
  let event;try{event=JSON.parse(line);}catch{continue;}
  if(event.kind!=='book'||!event.payload?.book)continue;
  const b=event.payload.book;if(!b.instrument?.endsWith(':YES'))continue;
  const prior=latest.get(String(b.marketId));if(!prior||Date.parse(b.receivedAt)>Date.parse(prior.receivedAt))latest.set(String(b.marketId),b);
}
const screeningAt=Date.now(), rows=[], missing=[];
for(const pair of stateNames){
  const [name,state]=pair.split(':'), market=catalogue.find(m=>m.title===`Will the Republican Party win the ${name} Senate?`);
  if(!market)continue;
  const prior=senate.races.get(`2020:${state}`), sp=president.states.get(`2024:${state}`), nat=president.national.get(2024);
  if(!prior||!sp){missing.push({state,marketId:String(market.market_id),reason:'missing_verified_prior_seat_or_president'});continue;}
  const priorNat=president.national.get(2016), x=[1,sp.margin-nat.margin,prior.margin-priorNat.margin,0];
  const structuralMargin=predict(report.model,x);
  // These declared sensitivities are assumptions, not current polling or confidence bounds.
  const variants=[-1,0,1].flatMap(incumbent=>[-.15,-.10,-.05,0,.05].map(nationalMargin=>({incumbent,nationalMargin,
    probability:probabilityFromMargin(predict(report.model,[...x.slice(0,3),incumbent])+nationalMargin,Math.hypot(report.residualRms,.06))})));
  const lower=Math.min(...variants.map(v=>v.probability)),upper=Math.max(...variants.map(v=>v.probability));
  const book=latest.get(String(market.market_id)), bestAsk=book?.asks?.reduce((a,b)=>!a||Number(b.price)<Number(a.price)?b:a,null), bestBid=book?.bids?.reduce((a,b)=>!a||Number(b.price)>Number(a.price)?b:a,null);
  const ageMs=book?screeningAt-Date.parse(book.receivedAt):null;
  rows.push({state,marketId:String(market.market_id),title:market.title,structuralMargin,scenarioMinimum:lower,scenarioMaximum:upper,
    ask:bestAsk?.price??null,askQuantity:bestAsk?.quantity??null,bid:bestBid?.price??null,bookReceivedAt:book?.receivedAt??null,bookAgeMs:ageMs,
    indicativeScenarioGap:bestAsk?lower-Number(bestAsk.price):null,quoteRecent:ageMs!==null&&ageMs>=0&&ageMs<=120000,
    productionEligible:false,reason:'Research screen: current entrant/source/contract review and calibrated executable edge remain unverified.'});
}
rows.sort((a,b)=>(b.indicativeScenarioGap??-Infinity)-(a.indicativeScenarioGap??-Infinity));
const result={at:new Date(screeningAt).toISOString(),modelHash:report.modelHash,tapeByteCutoff:byteCutoff,networkRequests:0,
  nationalMarginAssumptions:[-.15,-.10,-.05,0,.05],incumbentSensitivity:[-1,0,1],rows,missing,
  qualifiedProductionForecasts:0,profitabilityEstablished:false,fees:0};
fs.writeFileSync(`${base}/market-screen-affiliation-invariant.json`,JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({screened:rows.length,missing:missing.length,recentQuotes:rows.filter(r=>r.quoteRecent).length,top:rows.slice(0,6),qualifiedProductionForecasts:0,networkRequests:0}));
