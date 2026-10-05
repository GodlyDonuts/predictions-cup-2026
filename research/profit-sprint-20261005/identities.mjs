import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
const bytes=fs.readFileSync('data/markets.json'),markets=JSON.parse(bytes),universe=JSON.parse(fs.readFileSync('live-paper/price-universe.json'));
const selected=markets.filter(m=>/Texas|Iowa|Michigan|Alaska/.test(m.title)&&/Governor|Senate/.test(m.title)).map(m=>({marketId:m.id,title:m.title,exchangeId:m.exchanges[0].id,settlementDate:m.settlementDate,catalogueStatus:m.status}));
const predicates=[];
for(const folder of fs.readdirSync(path.join(runtime,'production-metadata'))){
 for(const file of fs.readdirSync(path.join(runtime,'production-metadata',folder))){
  if(!/^\d+\.json$/.test(file))continue;
  const source=fs.readFileSync(path.join(runtime,'production-metadata',folder,file)),j=JSON.parse(source);
  if(!selected.some(m=>m.marketId===j.marketId)||!j.root?.contract_details)continue;
  predicates.push({marketId:j.marketId,predicate:j.root.contract_details,rootHash:j.rootHash,receivedAt:j.receivedAt,sourceFile:'production-metadata/'+folder+'/'+file,sourceHash:createHash('sha256').update(source).digest('hex')});
 }
}
const report={schemaVersion:1,at:new Date().toISOString(),cachedCatalogueHash:createHash('sha256').update(bytes).digest('hex'),catalogueObservedAt:universe.catalogueObservedAt,marketCount:markets.length,settlementDates:[...new Set(markets.map(m=>m.settlementDate))],selected,predicates,executionAuthority:false,liveBookEvidence:false,notes:['Texas Senate is293/ex982 Democratic and294/ex983 Republican.181/ex870 is not that Senate market.','Texas Governor195/ex884 Democratic and196/ex885 Republican. Only195 cachedroot is available here;196 fullpredicate was not reverified.','Texas ballot includesPatDixonLIBGovernor andTedBrownLIBSenate; Democratic and Republican party-winner markets cannot be treated as exhaustive binary complements.','Separate same-exchangeYES/NO pairing mechanism is distinct from opposite-party markets. This research report does not verify a dispatch or execution quote.','All237 cachedmarkets shareNovember4 17:00UTC settlement; none is an election-outcome payout beforeOctober5 6pmEastern.']};
fs.writeFileSync('research/profit-sprint-20261005/market-identities.json',JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({markets:markets.length,settlementDates:report.settlementDates,predicateRecords:predicates.length}));
