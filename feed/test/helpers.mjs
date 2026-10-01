export const scope = {tournamentId:'11111111-1111-4111-8111-111111111111',marketId:'277',exchangeIds:['701','702']};
export const start = Date.parse('2026-10-01T21:00:00.000Z');
export const at = ms => new Date(start + ms).toISOString();
export function book(id = 701, sequence = 100, ms = 0, extra = {}) { return {exchangeId:id, asOf:{sequence,at:at(ms)}, nextExpiryAt:null, bids:[{price:.4,quantity:100}],asks:[{price:.6,quantity:200}],...extra}; }
export function trade(id = '101', sequence = 101, ms = 1, extra = {}) { return {id,sequence,exchangeId:701,marketId:277,tournamentId:scope.tournamentId,price:.5,quantity:10,executedAt:at(ms),...extra}; }
export function batch(revision = 1, previousRevision = 0, books = [book(701),book(702)], extra = {}) { return {trades:[],bookDirty:[],marketSettled:[],books,delivery:{model:'best-effort-authoritative-resync',correlationId:'synthetic-batch',revision,previousRevision,sourceSequenceFrom:100,sourceSequenceThrough:101},...extra}; }
export function snapshot(books = [book(701),book(702)]) { return {exchanges:[],contexts:[{type:'tournament',tournament:{id:scope.tournamentId},orderbook:{exchanges:books.map(b=>({...b,exchangeId:String(b.exchangeId),option:'YES',impliedProbability:.6})),overround:1.2,hasArbitrageOpportunity:false}}]}; }
export const clone = v => structuredClone(v);
