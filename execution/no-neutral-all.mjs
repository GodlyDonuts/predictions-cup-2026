import {createHash} from 'node:crypto';
import {parseNumberLexemes} from './exact-money-receipts.mjs';
import {payloadHash} from './core.mjs';
import {amount,money} from '../paper/fixed.mjs';

const need=(ok,code)=>{if(!ok)throw Error('NO_NEUTRAL_ALL_'+code);};
const equalPrincipal=['fullNotionalCost','effectiveEntryCost','netBuyingPowerImpact'];
const zeroEffects=['collateralSavings','guaranteedPayoutFloorAfter','outstandingAdvanceAfter','collateralRepayment','redemptionCredit'];
const fields=[...equalPrincipal,...zeroEffects,'relationshipIds','componentId'].sort();
// Read-only proof of the ACK's disclosed economics, not order/fill/position
// reconciliation. The original raw body and its hash are never rewritten.
export function inspectNoAckAllEconomics(receipt){
 need(receipt?.method==='POST'&&receipt.endpoint==='/orders'&&receipt.status===200&&typeof receipt.bodyText==='string','RECEIPT');
 const sourceBodyHash=createHash('sha256').update(receipt.bodyText).digest('hex');
 need(receipt.sourceBodyHash===undefined||receipt.sourceBodyHash===sourceBodyHash,'SOURCE_HASH');
 const {data,numbers}=parseNumberLexemes(receipt.bodyText);
 need(data&&Object.hasOwn(data,'all'),'MISSING_ALL');
 const proof={schemaVersion:1,mode:'strict_neutral_no_ack_economics_v1',sourceBodyHash,allState:data.all===null?'null':'neutral',executionAuthority:false,newSubmissionAuthorized:false,globalActivityVerified:false};
 if(data.all===null)return{...proof,verificationHash:payloadHash(proof)};
 const all=data.all;need(all&&typeof all==='object'&&!Array.isArray(all)&&Object.keys(all).sort().join('|')===fields.join('|'),'FIELDS');
 need(data.side==='no'&&data.action==='buy'&&data.quantity===1&&[0,1].includes(data.quantityTraded),'CANONICAL_NO_BUY');
 need(Array.isArray(all.relationshipIds)&&all.relationshipIds.length===0&&all.componentId===null,'RELATIONSHIP');
 const exact=path=>{const raw=numbers.get(JSON.stringify(path));need(typeof raw==='string'&&/^(0|[1-9]\d*)(\.\d{1,6})?$/.test(raw),'EXACT_NUMERIC_SOURCE');return amount(raw);};
 const principal=exact(['totalCost']);need(principal>=0n&&principal<=995000n&&principal%5000n===0n&&(data.quantityTraded===0?principal===0n:principal>0n),'PRINCIPAL');
 for(const field of equalPrincipal)need(exact(['all',field])===principal,'PRINCIPAL_MISMATCH');
 for(const field of zeroEffects)need(exact(['all',field])===0n,'NONZERO_EFFECT');
 const result={...proof,ackNotional:money(principal),fullNotionalCost:money(principal),effectiveEntryCost:money(principal),netBuyingPowerImpact:money(principal),relationshipIds:[],componentId:null,collateralSavings:'0.000000',guaranteedPayoutFloorAfter:'0.000000',outstandingAdvanceAfter:'0.000000',collateralRepayment:'0.000000',redemptionCredit:'0.000000'};
 return{...result,verificationHash:payloadHash(result)};
}
