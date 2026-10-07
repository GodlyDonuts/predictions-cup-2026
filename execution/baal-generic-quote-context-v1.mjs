// The collector's bulk rows omit tournamentId. Fill only missing context from
// the already checked envelope; never overwrite foreign or future source data.
import {partyWinnerQuoteHash} from '../live-paper/party-winner-no-screen-v1.mjs';
export function bindBaalGenericQuoteContextV1({hints,tournamentId,universeHash}){
 if(hints?.tournamentId!==tournamentId||hints?.universeHash!==universeHash||!Array.isArray(hints.quoteCaptures))throw Error('GENERIC_QUOTE_ENVELOPE');
 return hints.quoteCaptures.map(q=>{const value={...q,tournamentId:q.tournamentId??tournamentId,knownAt:q.knownAt??q.receivedAt};return{...value,quoteHash:q.quoteHash??partyWinnerQuoteHash(value)};});
}
