// Two different party winners in the same single-seat race cannot both win.
// Buying NO on both pays at least one unit per matched pair, even if a third party wins.
export function raceIdentity(node) {
  const d = node?.contract_details;
  if (node?.node_type !== 'contract' || node.contract_type !== 'Election Outcome' ||
      d?.resolutionType !== 'Party Winner' || d?.raceStage !== 'General' ||
      !d.raceId || !d.stageId || !d.electionDate || !d.winnerName) return null;
  return JSON.stringify([d.raceId, d.stageId, d.electionDate, node.settlement_date, d.resolutionType]);
}

export function verifiedPairs(markets, nodes) {
  const groups = new Map();
  for (const m of markets) {
    if (m.status !== 'open' || m.isComposite || m.exchanges?.length !== 1) continue;
    const node = nodes[m.id]?.root;
    const key = raceIdentity(node);
    if (!key || node.settled_with != null) continue;
    const party = node.contract_details.winnerName;
    // Conservative initial scope: recognized opposing parties only, distinct contracts.
    if (!['Democratic Party', 'Republican Party'].includes(party)) continue;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push({marketId:m.id, exchangeId:m.exchanges[0].id, title:m.title, party});
  }
  return [...groups.values()].filter(g => g.length === 2 && g[0].party !== g[1].party);
}

export function toMills(value) {
  const text=String(value);
  if(!/^\d+(\.\d{1,3})?$/.test(text))throw new Error('Expected a nonnegative price or amount with at most three decimal places');
  const [whole,frac='']=text.split('.');const result=Number(whole)*1000+Number(frac.padEnd(3,'0'));
  if(!Number.isSafeInteger(result))throw new Error('Amount exceeds exact integer accounting range');
  return result;
}
export const formatMills=value=>`${Math.floor(value/1000)}.${String(value%1000).padStart(3,'0')}`;
function noOffer(book) {
  const bids = book?.bids?.filter(b => Number.isFinite(b.price) && b.quantity >= 1 && b.price > 0 && b.price < 1);
  if (!bids?.length) return null;
  const bid = bids.reduce((a,b) => a.price > b.price ? a : b);
  // NO is bought against a YES bid at its complement. Round cost upward to the API tick.
  const priceMills=Math.ceil((1000-toMills(bid.price))/5)*5;
  return {priceMills, quantity:Math.floor(bid.quantity)};
}

export function planPair(pair, books, cfg, cash, positions = []) {
  if (pair.length !== 2 || pair[0].exchangeId === pair[1].exchangeId) return null;
  // Avoid interacting with the user's existing holdings or previously filled packages.
  if (positions.some(p => pair.some(m => m.exchangeId === p.exchangeId) && p.quantity !== 0)) return null;
  const offers = pair.map(m => noOffer(books[m.exchangeId]));
  if (offers.some(o => !o)) return null;
  const cost = offers[0].priceMills + offers[1].priceMills;
  const edge = 1000 - cost;
  if (edge < toMills(cfg.minimumEdge) || cost <= 0) return null;
  const quantity = Math.floor(Math.min(cfg.maxSharesPerLeg, ...offers.map(o => o.quantity * 0.25),
    toMills(cfg.maxPackageCost) / cost, Math.max(0, toMills(cash) - toMills(cfg.cashReserve)) / cost));
  if (quantity < 1) return null;
  return {pair, quantity, unitCost:formatMills(cost), minimumPayout:quantity, minimumProfit:formatMills(edge*quantity),
    worstCaseCost:formatMills(cost*quantity),
    legs:pair.map((m,i) => ({exchangeId:m.exchangeId,side:'no',action:'buy',quantity,
      price:Number(formatMills(offers[i].priceMills)),tournamentId:cfg.tournamentId}))};
}

