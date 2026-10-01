import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {setTimeout as sleep} from 'node:timers/promises';
import {Api,loadKey} from './api.mjs';
import {metadataPairs,planPair} from './strategy.mjs';

process.chdir(path.dirname(fileURLToPath(import.meta.url)));
if(process.argv.includes('--live'))throw new Error('Live execution disabled: dot owns execution. Resolve ownership and implement a shared lease before enabling trading.');
const cfg=JSON.parse(fs.readFileSync('config.json','utf8'));
const once=process.argv.includes('--once');
const sample=process.argv.includes('--sample');
fs.mkdirSync('data',{recursive:true});fs.mkdirSync('logs',{recursive:true});
fs.mkdirSync('data/sources',{recursive:true});
const lockPath=path.join(process.env.LOCALAPPDATA ?? '.', 'SigTradingCup','bot.lock');
fs.mkdirSync(path.dirname(lockPath),{recursive:true});
let lock;
try {lock=fs.openSync(lockPath,'wx');fs.writeSync(lock,String(process.pid));}
catch {throw new Error('Another local scanner may be running. Verify its PID before removing LocalAppData/SigTradingCup/bot.lock.');}
function cleanup(){if(lock!==undefined){fs.closeSync(lock);fs.unlinkSync(lockPath);lock=undefined;}}
process.on('exit',cleanup);
let stopping=false;
process.on('SIGINT',()=>{stopping=true;});process.on('SIGTERM',()=>{stopping=true;});
const api=new Api(cfg.baseUrl,loadKey(),cfg.readIntervalMs);
const log=(event,fields={})=>{const row={at:new Date().toISOString(),event,...fields};fs.appendFileSync('logs/bot.jsonl',JSON.stringify(row)+'\n');console.log(JSON.stringify(row));};
const save=(name,data)=>{const tmp=`data/${name}.tmp`;fs.writeFileSync(tmp,JSON.stringify(data,null,2));fs.renameSync(tmp,`data/${name}.json`);};
const read=(name,fallback)=>fs.existsSync(`data/${name}.json`)?JSON.parse(fs.readFileSync(`data/${name}.json`,'utf8')):fallback;
const shouldStop=()=>stopping||fs.existsSync('STOP');
const scope=()=>`tournamentId=${cfg.tournamentId}`;
let snapshotSequence=0;
async function source(endpoint) {
  const requestStartedAt=new Date().toISOString();const response=await api.request(endpoint);
  const receivedAt=new Date().toISOString();
  fs.writeFileSync(`data/sources/${receivedAt.replaceAll(':','-')}-${snapshotSequence++}.json`,JSON.stringify({endpoint,requestStartedAt,receivedAt,response},null,2),{flag:'wx'});
  return {response,requestStartedAt,receivedAt};
}

async function cycle() {
  const tournament=(await source(`/tournaments/${cfg.tournamentSlug}`)).response;
  if(tournament.id!==cfg.tournamentId || tournament.currencyName!=='SUSQies')throw new Error('Tournament identity mismatch');
  const portfolioRead=await source(`/tournaments/${cfg.tournamentSlug}/portfolio/positions`);
  const portfolio=portfolioRead.response;
  save('portfolio',{...portfolio,observedAt:portfolioRead.receivedAt,cash:tournament.myBalance});
  const orders=await api.pages(`/orders?status=open&limit=200&${scope()}`);
  save('orders',{observedAt:new Date().toISOString(),orders});
  const markets=await api.pages(`/markets?status=open&limit=100&${scope()}`);
  save('markets',markets);
  const nodes=read('nodes',{});
  let candidates=markets.filter(m=>/^Will the (Democratic|Republican) Party win /.test(m.title)&&!m.isComposite&&m.exchanges.length===1);
  if(sample)candidates=candidates.slice(0,20);
  // Concurrent network reads use one paced admission queue within this process.
  let index=0,loaded=0;
  const verification=await Promise.allSettled(Array.from({length:4},async()=>{
    while(index<candidates.length&&!shouldStop()){
      const m=candidates[index++];
      if(nodes[m.id]?.observedAt && Date.now()-Date.parse(nodes[m.id].observedAt)<12*3600000)continue;
      const s=await source(`/markets/${m.id}/nodes?${scope()}`);
      nodes[m.id]={...s.response,observedAt:s.receivedAt};save('nodes',nodes);loaded++;
      if(loaded%25===0)log('contract_verification',{verified:loaded,total:candidates.length});
    }
  }));
  const failed=verification.find(r=>r.status==='rejected');if(failed)throw failed.reason;
  if(shouldStop())return;
  const pairs=metadataPairs(sample?candidates:markets,nodes);
  save('pairs',pairs);
  log('scan_start',{mode:'read_only',balance:tournament.myBalance,markets:markets.length,metadataPairs:pairs.length});
  const opportunities=[];let scanned=0;
  for(const pair of pairs){
    if(shouldStop())return;
    const books={};const observationWindow=[];
    for(const leg of pair){
      const s=await source(`/exchanges/${leg.exchangeId}/orderbook?depth=5&${scope()}`);
      books[leg.exchangeId]=s.response;observationWindow.push({exchangeId:leg.exchangeId,requestStartedAt:s.requestStartedAt,receivedAt:s.receivedAt,asOf:s.response.asOf});
    }
    const plan=planPair(pair,books,cfg,tournament.myBalance,portfolio.positions,orders);
    if(plan){
      const candidate={...plan,observationWindow,feesVerified:false,eligibleForExecution:false,
        limitations:['Exact settlement rules and exclusive outcomes are unverified','Sequential snapshots can be stale','Partial fills can create directional exposure','Fee schedule and exceptional settlement cases need verification']};
      opportunities.push(candidate);
      log('research_candidate',{titles:pair.map(p=>p.title),classification:plan.classification,quantity:plan.quantity,cost:plan.worstCaseCost,quoteDifference:plan.quoteDifference});
    }
    scanned++;if(scanned%20===0)log('book_scan_progress',{scanned,total:pairs.length});
  }
  save('opportunities',opportunities);
  save('status',{at:new Date().toISOString(),mode:'read_only',sample,balance:tournament.myBalance,markets:markets.length,contractsScanned:candidates.length,pairs:pairs.length,opportunities:opportunities.length,openOrders:orders.length});
  log('scan_complete',{opportunities:opportunities.length,ordersSubmitted:0});
}

try {
  log('started',{mode:'read_only',pid:process.pid});
  do{await cycle();if(once||shouldStop())break;await sleep(cfg.cycleIntervalMs);}while(!shouldStop());
  log('stopped');
}catch(error){log('stopped_error',{message:error.message});process.exitCode=1;}
