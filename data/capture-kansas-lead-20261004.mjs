import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {sha} from '../live-paper/native-dip-proof-v1.mjs';
import {amount,money,TICK} from '../paper/fixed.mjs';
import {gunzipSync} from 'node:zlib';
import {createHash} from 'node:crypto';

const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),dir=path.join(runtime,'research-sources','kansas-20261004-'+Date.now());fs.mkdirSync(dir,{recursive:true,mode:0o700});
const sources=[];
for(const [name,url]of [['emerson','https://emersoncollegepolling.com/kansas-2026/'],['ddhq-forecast','https://votes.decisiondeskhq.com/races/2026-11-03/kansas-us-senate-general-election/forecast'],['ddhq-polling','https://votes.decisiondeskhq.com/races/2026-11-03/kansas-us-senate-general-election/polling']]){
 const response=await fetch(url,{redirect:'error',signal:AbortSignal.timeout(20000)});assert(response.ok,'Primary public source unavailable');const bytes=Buffer.from(await response.arrayBuffer());assert(bytes.length<10*1024*1024);fs.writeFileSync(path.join(dir,name+'.html'),bytes,{flag:'wx',mode:0o600});
 sources.push({url,contentHash:sha(bytes),bytes:bytes.length,retrievedAt:new Date().toISOString(),role:name});
}
const cached='data/cup-identity-readonly-20261004.json';
let identityMap;
if(fs.existsSync(cached))identityMap=JSON.parse(fs.readFileSync(cached));
else{
 const compressed=fs.readFileSync('research/crossvenue_screen_2026-10-02/cup_ui_identity_map_2026-10-02.json.gz');
 assert.equal(createHash('sha1').update(Buffer.concat([Buffer.from('blob '+compressed.length+'\0'),compressed])).digest('hex'),'9581ac06796235a0a6ab425b4e944727b9c93440');identityMap=JSON.parse(gunzipSync(compressed));
}
const identities=identityMap.markets.filter(x=>['356','357'].includes(String(x.market_id)));
const quotes=JSON.parse(fs.readFileSync('data/kansas-verified-quotes-20261004.json'));
assert.equal(identities.length,2);assert.equal(quotes.quotes.length,2);
const republicanQuote=quotes.quotes.find(x=>String(x.marketId)==='357');
const democraticQuote=quotes.quotes.find(x=>String(x.marketId)==='356');
const pointSurplus=amount('0.770000')-amount(republicanQuote.bestAsk.toFixed(6))-TICK;
const pairedCost=amount('1.000000')-amount(republicanQuote.bestBid.toFixed(6))+amount('1.000000')-amount(democraticQuote.bestBid.toFixed(6))+2n*TICK;
const record={schemaVersion:1,recordedAt:new Date().toISOString(),status:'exact_candidate_lead_requires_forecast_and_rule_review',sources,
 identities:identities.map(x=>({marketId:String(x.market_id),title:x.title,party:x.party,url:x.url,identityObservedAt:x.observed_at,resolutionVerified:false})),
 indicativeQuotes:quotes.quotes,quotePrefix:quotes.prefix,quoteHostOutputHash:quotes.hostOutputHash,
 sourceObservations:[
  {role:'emerson',fieldDates:['2026-09-15','2026-09-16'],sample:'750 likely voters',voteSharesPercent:{Hamilton:45,Marshall:43,Graham:4,undecided:8},credibilityIntervalPercent:3.6,observationBasis:'Primary page reviewed through web tool; content archive retained locally',winProbability:null},
  {role:'ddhq-forecast',publisherUpdatedText:'Oct 2, 2:02 PM',publisherTimezoneKnown:false,winProbability:{Marshall:0.77,Hamilton:0.23},modelComponentsPercent:{fundamentals:73,polls:2,markets:25},observedPollingAverage:{dateText:'Oct 4, 11:33 AM EDT',Hamilton:45.60,Marshall:45.40},observationBasis:'Primary forecast page reviewed; polling-average update does not refresh the older race probability'},
  {role:'ddhq-polling',displayedMarshallWinProbability:0.83,publisherUpdateExposed:false,observationBasis:'Primary polling-page footer reviewed; distinct cached display is not a second independent model'}],
 scenarioOnly:{RepublicanYesAsk:republicanQuote.bestAsk,adverseTick:0.005,ddhqPointMinusCushionedAsk:money(pointSurplus),interpretation:'Arithmetic on the indicative quote and one published model point, not a confidence-bound edge, depth-confirmed fill or booked gain'},
 pairedQuoteLead:{canonicalNoCostWithTwoCushions:money(pairedCost),conditionalOrdinaryMutualExclusionSurplus:money(amount('1.000000')-pairedCost),mutualExclusionReviewed:false,refundCompatibilityReviewed:false,depthConfirmed:false,interpretation:'Separate indicative dual-NO lead; winner-party/fusion/admin rules and all leg depth must be verified. No guaranteed floor, fill, batch atomicity or profit inferred.'},
 captureObservationBoundary:'Source observations came from web-tool renderings. Raw HTML captures can differ from cached rendering; hashes prove archive integrity, not automated verification of the observation fields.',
 unresolved:['Exact Cup individual resolution conditions and party binding need current review','Candidate-list link returned403 through web tool; general ballot not independently read','Polling-page83% versus forecast-page77% display timestamps are unresolved; do not blend them as independent forecasts','Poll vote share and election win probability are different quantities','Fundamentals, polls and the25% market component are correlated inputs','No conservative calibrated probability interval or prospective fill/depth evidence established'],
 method:null,probabilityInterval:null,newPaperBank:false,competitionOrders:0,sigApiRequests:0,publicResearchHttpRequests:3,profitabilityEstablished:false};
fs.writeFileSync('data/kansas-source-lead-20261004.json',JSON.stringify(record)+'\n',{flag:'wx'});
console.log(JSON.stringify({status:record.status,sourceCaptures:sources.map(x=>({role:x.role,bytes:x.bytes,contentHash:x.contentHash})),cupMarkets:record.identities.map(x=>x.marketId),pointScenarioSurplus:record.scenarioOnly.ddhqPointMinusCushionedAsk,depthConfirmed:false,bankCreated:false,profitabilityEstablished:false}));
