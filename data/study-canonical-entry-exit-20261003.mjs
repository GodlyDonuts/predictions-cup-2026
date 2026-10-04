import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {createBookPublicationState,applyBookPublication} from '../live-paper/book-publication-v2.mjs';
import {digest} from '../live-paper/model.mjs';import {evaluateCanonicalNoCapacity} from '../live-paper/canonical-no-capacity.mjs';
import {studyCapacityPaths} from '../live-paper/canonical-entry-exit-study.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),sha=b=>createHash('sha256').update(b).digest('hex');
const a=JSON.parse(fs.readFileSync(path.join(r,'canonical-capacity-active.json'))),dir=path.join(r,'canonical-capacity-cohorts',a.cohort),bankBytes=fs.readFileSync(path.join(dir,'bank.json')),bank=JSON.parse(bankBytes);
if(digest(bank)!==a.bankHash)throw Error('Bank identity mismatch');
const complete=file=>{const b=fs.readFileSync(file);return b.subarray(0,b.lastIndexOf(10)+1);},rows=b=>b.length?b.toString().trimEnd().split('\n').map(JSON.parse):[];
const capacityBytes=complete(path.join(dir,'journal.jsonl')),capacity=rows(capacityBytes);let previous=a.bankHash,sequence=0;
for(const row of capacity){const{hash,...body}=row;if(body.sequence!==++sequence||body.previous!==previous||body.bankHash!==a.bankHash||digest(body)!==hash)throw Error('Capacity chain mismatch');previous=hash;}
const tapeBytes=complete(path.join(r,'anonymous-trades.jsonl')),pages=rows(tapeBytes);previous='';sequence=0;
for(const row of pages){const{hash,...body}=row;if(body.sequence!==++sequence||body.previous!==previous||sha(JSON.stringify(body))!==hash)throw Error('Tape chain mismatch');previous=hash;}
const sourceBytes=complete(path.join(r,'public-books.jsonl'));if(sha(sourceBytes.subarray(0,bank.sourcePrefixBytes))!==bank.sourcePrefixHash)throw Error('Original prefix mismatch');
const wanted=new Set(capacity.map(x=>x.pair.sourceSequence)),frames=new Map();let s=createBookPublicationState(JSON.parse(fs.readFileSync(new URL('../live-paper/price-universe.json',import.meta.url))));
for(const row of rows(sourceBytes)){s=applyBookPublication(s,row);if(wanted.has(s.sequence))frames.set(s.sequence,{hash:s.hash,books:bank.review.legs.map(l=>s.books[l.exchangeId+':YES'])});}
const actual=capacity.map(row=>{const frame=frames.get(row.pair.sourceSequence);if(!frame||frame.hash!==row.pair.sourceHash||frame.books.some((b,i)=>b?.versionKey!==row.pair.versions[i]))throw Error('Frame binding mismatch');
 const expected=evaluateCanonicalNoCapacity({review:bank.review,books:frame.books,account:row.accountEligibility,now:row.at,policy:bank.policy});if(digest(expected)!==digest(row.result))throw Error('Original frame result replay mismatch');
 return {at:row.at,sourceSequence:row.pair.sourceSequence,sourceHash:row.pair.sourceHash,versions:row.pair.versions,books:frame.books,accountEligibility:row.accountEligibility,result:row.result};});
const trades=pages.flatMap(row=>row.event.trades.map(t=>({...t,tournamentId:row.event.tournamentId,marketId:row.event.marketId,exchangeId:row.event.exchangeId,firstAvailableAt:row.event.receivedAt})));
const result={at:new Date().toISOString(),cohort:a.cohort,bankHash:a.bankHash,sources:{publicBooks:{bytes:sourceBytes.length,sha256:sha(sourceBytes),sequence:s.sequence,tailHash:s.hash},capacity:{bytes:capacityBytes.length,sha256:sha(capacityBytes),rows:capacity.length},tradeTape:{bytes:tapeBytes.length,sha256:sha(tapeBytes),pages:pages.length,trades:trades.length}},...studyCapacityPaths({frames:actual,trades})};
const output=process.argv[2]||'data/canonical-entry-exit-diagnostic-20261003.json';fs.writeFileSync(output,JSON.stringify(result,null,2));console.log(JSON.stringify({output,at:result.at,frames:result.frames,usableFrames:result.usableFrames,positiveAnchors:result.positiveAnchors,anchorsWithLaterCompleteDepth:result.anchorsWithLaterCompleteDepth,positiveLaterDepthMarks:result.anchorsWithAnyPositiveLaterDepthMark,makerBothLegsThrough:result.makerBothLegsThrough,makerOneLegThrough:result.makerOneLegThrough,profitabilityEstablished:false}));
