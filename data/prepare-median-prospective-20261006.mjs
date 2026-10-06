import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
const sha=b=>createHash('sha256').update(b).digest('hex');
const rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime');
const release=JSON.parse(fs.readFileSync('data/median-prospective-build-20261006.json'));
const cohort='baal-median-prospective-20261006-v1';
const dir=path.join(rt,'median-paper-cohorts',cohort);
if(fs.existsSync(dir))throw Error('Refuse to replace a frozen cohort');
const binary=fs.readFileSync(release.binaryPath);
if(sha(binary)!==release.binaryHash||!binary.includes(Buffer.from(release.engineHash)))throw Error('Compiled identity mismatch');
const sources=release.sourcePins.map(p=>{const source=path.resolve(p.relativePath),bytes=fs.readFileSync(source);if(sha(bytes)!==p.sha256)throw Error('Changed checked source');return {...p,source,bytes};});
const nativePath=path.join(dir,'bin','baal-median-dip-replay.exe');
const pins=[];
fs.mkdirSync(path.dirname(nativePath),{recursive:true});
fs.writeFileSync(nativePath,binary,{flag:'wx'});
pins.push({path:nativePath,sha256:release.binaryHash});
for(const p of sources){const target=path.join(dir,'sources',p.relativePath);fs.mkdirSync(path.dirname(target),{recursive:true});fs.writeFileSync(target,p.bytes,{flag:'wx'});pins.push({path:target,sha256:p.sha256});}
const dll=path.join(process.env.LOCALAPPDATA,'SigTradingCup/rust-toolchain/rustup/toolchains/stable-x86_64-pc-windows-gnullvm/bin/libunwind.dll');
const dllBytes=fs.readFileSync(dll),dllTarget=path.join(dir,'bin/libunwind.dll');fs.writeFileSync(dllTarget,dllBytes,{flag:'wx'});pins.push({path:dllTarget,sha256:sha(dllBytes)});
function anchor(file){const fd=fs.openSync(file,'r'),n=fs.fstatSync(fd).size,b=Buffer.alloc(n);let offset=0;while(offset<n){const read=fs.readSync(fd,b,offset,n-offset,offset);if(!read)throw Error('Truncated capture');offset+=read;}fs.closeSync(fd);const end=b.lastIndexOf(10)+1;if(!end)throw Error('Missing complete capture');const prefix=b.subarray(0,end),lastStart=prefix.lastIndexOf(10,end-2)+1,last=JSON.parse(prefix.subarray(lastStart,end));return {bytes:end,sha256:sha(prefix),sequence:last.sequence,hash:last.hash,lastAt:last.at};}
const publicSourcePath=path.join(rt,'public-books.jsonl'),accountSourcePath=path.join(rt,'account-eligibility.jsonl'),priceSourcePath=path.join(rt,'market-prices.jsonl');
const publicPrefix=anchor(publicSourcePath),accountPrefix=anchor(accountSourcePath),featurePrefix=anchor(priceSourcePath);
const cutoff=new Date().toISOString();
for(const a of [publicPrefix,accountPrefix,featurePrefix]){if(Date.parse(a.lastAt)>Date.parse(cutoff))throw Error('Future capture anchor');delete a.lastAt;}
const old=JSON.parse(fs.readFileSync(path.join(rt,'settlement-portfolio-v3-cohorts/kansas-receipt-baseline-20261006-v3/bank.json')));
const bank={schemaVersion:1,mode:'fixed_rolling_median_prospective_paper_v1',name:cohort,cutoff,universe:old.universe,publicPrefix,accountPrefix,featurePrefix,featurePolicy:release.featurePolicy,thresholds:[1,2,4],policy:release.fixedPolicy,codeIdentity:{engineHash:release.engineHash,binaryHash:release.binaryHash,sources:Object.fromEntries(release.sourcePins.map(p=>[p.relativePath,p.sha256]))}};
const bankPath=path.join(dir,'bank.json'),bankBytes=Buffer.from(JSON.stringify(bank,null,2)+'\n');fs.writeFileSync(bankPath,bankBytes,{flag:'wx'});
// The native digest preserves JSON insertion order, as does this exact serialization.
const bankHash=sha(Buffer.from(JSON.stringify(bank)));
const config={cohortDir:dir,bankPath,bankFileHash:sha(bankBytes),bankHash,publicSourcePath,accountSourcePath,priceSourcePath,stopPaths:[path.join(rt,'STOP'),path.join(rt,'COLLECTOR_STOP')],codePins:pins,entryEnabled:true,entryDrainPath:path.join(dir,'ENTRY_DRAIN')};
const configPath=path.join(dir,'observer-config.json');fs.writeFileSync(configPath,JSON.stringify(config,null,2)+'\n',{flag:'wx'});
const evidence={schemaVersion:1,at:cutoff,cohort,bankHash,engineHash:release.engineHash,binaryHash:release.binaryHash,publicPrefix,accountPrefix,featurePrefix,thresholds:[1,2,4],configRelative:path.relative(rt,configPath),binaryRelative:path.relative(rt,nativePath),pins:pins.length,fees:0,primedHistoryOnly:true,oldFillsImported:false,sessionLaunched:false,competitionOrders:0,networkRequests:0,profitabilityEstablished:false};
fs.writeFileSync('data/median-prospective-prepared-20261006.json',JSON.stringify(evidence,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(evidence));
