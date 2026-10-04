import {journalLines} from './journal-lines.mjs';
import {payloadHash} from '../execution/ledger.mjs';

// A pair host sees only its own markets. Sealing unrelated records is safe;
// sealing a relevant book that the prior wallet never consumed is not.
export function paperBookContinuationFrontier({file,endByte,priorState,legs}) {
 const need=(v,c)=>{if(!v)throw Error('PAPER_BOOK_CONTINUATION_'+c);};
 need(Number.isSafeInteger(priorState.sourceSequence)&&priorState.sourceSequence>0,'FRONTIER');
 let sequence=0,previous='',found=false,ignored=0;
 const markets=new Set(legs.map(l=>l.marketId));
 for(const line of journalLines(file,{endByte})){
  const row=JSON.parse(line),{hash,...body}=row;
  need(row.sequence===++sequence&&row.previousHash===previous&&payloadHash(body)===hash,'CHAIN');previous=hash;
  if(sequence===priorState.sourceSequence){need(hash===priorState.sourceHash,'FRONTIER_CHANGED');found=true;}
  if(sequence>priorState.sourceSequence){
   need(row.kind!=='book'||!markets.has(row.payload?.book?.marketId),'UNCONSUMED_RELEVANT_BOOK');ignored++;
  }
 }
 need(found,'FRONTIER_TRUNCATED');
 return{consumedSequence:priorState.sourceSequence,consumedHash:priorState.sourceHash,sealedSequence:sequence,sealedHash:previous,unrelatedOrBoundarySuffix:ignored};
}
