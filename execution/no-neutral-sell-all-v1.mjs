import {createHash} from 'node:crypto';
import {parseNumberLexemes} from './exact-money-receipts.mjs';
import {payloadHash} from './core.mjs';
import {amount,money} from '../paper/fixed.mjs';

const need=(ok,c)=>{if(!ok)throw Error('NO_NEUTRAL_SELL_ALL_'+c);};
const sameProceeds=['fullNotionalCost','effectiveEntryCost'];
const zeroEffects=['collateralSavings','guaranteedPayoutFloorAfter','outstandingAdvanceAfter','collateralRepayment','redemptionCredit'];
const fields=[...sameProceeds,...zeroEffects,'netBuyingPowerImpact','relationshipIds','componentId'].sort();
// Observed canonical one-share NO SELL ACK 2026-10-06T15:22:50.772Z, original
// raw SHA92110a984282511f5ef57ad7068aacc081157fbebb7c1a4f386433b97890d976.
// Its positive effectiveEntryCost is preserved, never interpreted as a debit.
// Negative netBuyingPowerImpact reports released buying power. This helper is
// ACK disclosure evidence ONLY; terminal fills/holdings/cash still need proof.
export function inspectNoSellAckAllEconomics(receipt){need(receipt?.method==='POST'&&receipt.endpoint==='/orders'&&receipt.status===200&&typeof receipt.bodyText==='string','RECEIPT');const sourceBodyHash=createHash('sha256').update(receipt.bodyText).digest('hex');need(receipt.sourceBodyHash===undefined||receipt.sourceBodyHash===sourceBodyHash,'SOURCE_HASH');const {data,numbers}=parseNumberLexemes(receipt.bodyText);need(data&&Object.hasOwn(data,'all'),'MISSING_ALL');const base={schemaVersion:1,mode:'observed_neutral_no_sell_ack_economics_v1',sourceBodyHash,allState:data.all===null?'null':'observed_neutral_sell',executionAuthority:false,newSubmissionAuthorized:false,globalActivityVerified:false};if(data.all===null)return{...base,verificationHash:payloadHash(base)};
 const all=data.all;need(all&&typeof all==='object'&&!Array.isArray(all)&&Object.keys(all).sort().join('|')===fields.join('|'),'FIELDS');need(data.side==='no'&&data.action==='sell'&&data.quantity===1&&data.quantityTraded===1&&data.remainingQuantity===0&&data.open===false,'OBSERVED_CANONICAL_FILLED_NO_SELL');need(Array.isArray(all.relationshipIds)&&all.relationshipIds.length===0&&all.componentId===null,'RELATIONSHIP');
 const exact=path=>{const raw=numbers.get(JSON.stringify(path));need(typeof raw==='string'&&/^-?(0|[1-9]\d*)(\.\d{1,6})?$/.test(raw),'EXACT_NUMERIC_SOURCE');return amount(raw);};for(const k of ['quantity','quantityTraded','remainingQuantity'])need(exact([k])===BigInt(data[k])*1000000n,'EXACT_QUANTITY');const proceeds=exact(['totalCost']);need(proceeds>0n&&proceeds<=995000n&&proceeds%5000n===0n,'PROCEEDS');for(const k of sameProceeds)need(exact(['all',k])===proceeds,'PROCEEDS_MISMATCH');need(exact(['all','netBuyingPowerImpact'])===-proceeds,'RELEASE_SIGN_OR_PROCEEDS');for(const k of zeroEffects)need(exact(['all',k])===0n,'NONZERO_EFFECT');
 const result={...base,ackNotional:money(proceeds),fullNotionalCost:money(proceeds),effectiveEntryCost:money(proceeds),netBuyingPowerImpact:money(-proceeds),relationshipIds:[],componentId:null,collateralSavings:'0.000000',guaranteedPayoutFloorAfter:'0.000000',outstandingAdvanceAfter:'0.000000',collateralRepayment:'0.000000',redemptionCredit:'0.000000',cashDeltaInterpretation:'requires_separate_complete_scoped_no_sell_reconciliation'};return{...result,verificationHash:payloadHash(result)};}
