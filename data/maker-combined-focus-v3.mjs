import fs from 'node:fs';import path from 'node:path';import {Journal} from '../live-paper/transport-runtime.mjs';import {atomicJson,payloadHash} from '../execution/ledger.mjs';import {makerSelectedWalletReportV2} from '../paper/maker-selected-wallet-v2.mjs';
import {payloadHash as legacyHashOf} from '../execution/core.mjs';
const need=(v,c)=>{if(!v)throw Error('MAKER_COMBINED_FOCUS_'+c);};
export function combineMakerFocusV3({legacyBank,legacyResult,makerBank,makerState}){
 const {bankHash:legacyHash,...legacy}=legacyBank,{bankHash:makerHash,...maker}=makerBank;
 need(legacyHashOf(legacy)===legacyHash&&payloadHash(maker)===makerHash,'SOURCE_BANK');need(legacyResult.bankHash===legacyHash&&legacyResult.cutoff===legacy.cutoff&&legacyResult.competitionOrders===0&&makerState.bankHash===makerHash,'SOURCE_RESULT');
 need(legacy.executionAuthority===false&&maker.executionAuthority===false&&Array.isArray(legacy.policies)&&Array.isArray(legacyResult.cohorts),'SOURCE_POLICY');
 for(const c of legacyResult.cohorts)need(legacy.policies.some(p=>payloadHash(p)===payloadHash(c.policy))&&Array.isArray(c.unresolvedQuotes),'LEGACY_MEMBERSHIP');
 const body={schemaVersion:1,mode:'read_only_combined_coverage_focus',cutoff:legacy.cutoff,policies:[...legacy.policies,maker.policy],sourceBanks:[legacyHash,makerHash],executionAuthority:false,networkRequests:0};const bank={...body,bankHash:legacyHashOf(body)};
 const cohort={id:maker.cohort,policy:maker.policy,unresolvedQuotes:structuredClone(makerState.wallet.wallet.quotes.filter(q=>!q.closedAt)),report:makerSelectedWalletReportV2(makerState.wallet,maker.policy),sourceBankHash:makerHash};
 const result={bankHash:bank.bankHash,cutoff:bank.cutoff,cohorts:[...legacyResult.cohorts.map(c=>({id:c.id??c.name??('legacy-'+payloadHash(c.policy).slice(0,12)),policy:structuredClone(c.policy),unresolvedQuotes:structuredClone(c.unresolvedQuotes),report:{positions:structuredClone(c.report?.positions??{})},sourceBankHash:legacyHash})),cohort],competitionOrders:0,executionAuthority:false,sourceBindings:{legacyResult:payloadHash(legacyResult),makerState:payloadHash(makerState)},readOnlyCoverageProjection:true};return{bank,result};
}
export function publishMakerCombinedFocusV3({runtimeDir,workspace=process.cwd()}){
 const configFile=path.join(runtimeDir,'maker-combined-focus-config-v1.json');if(!fs.existsSync(configFile))return null;const config=JSON.parse(fs.readFileSync(configFile));
 need(/^[a-z][a-z0-9-]{0,79}$/.test(config.legacy.prefix)&&/^\d{8}$/.test(config.legacy.stamp)&&/^[a-z][a-z0-9-]{1,70}$/.test(config.makerCohort),'CONFIG');
 const base=path.join(workspace,'data',config.legacy.prefix),legacyBank=JSON.parse(fs.readFileSync(base+'-bank-'+config.legacy.stamp+'.json')),legacyResult=JSON.parse(fs.readFileSync(base+'-results-'+config.legacy.stamp+'.json'));
 need(legacyBank.bankHash===config.legacy.bankHash,'LEGACY_HASH');const dir=path.join(runtimeDir,'maker-prospective-cohorts',config.makerCohort),makerBank=JSON.parse(fs.readFileSync(path.join(dir,'bank.json'))),journal=new Journal(path.join(dir,'journal.jsonl'));if(!journal.last)return null;need(makerBank.bankHash===config.makerBankHash,'MAKER_HASH');
 const out=combineMakerFocusV3({legacyBank,legacyResult,makerBank,makerState:journal.last.state});atomicJson(path.join(workspace,'data/maker-combined-focus-bank-20261012.json'),out.bank);atomicJson(path.join(workspace,'data/maker-combined-focus-results-20261012.json'),out.result);return{bankHash:out.bank.bankHash,cohorts:out.result.cohorts.length};
}


