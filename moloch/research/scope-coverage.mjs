export function scopeCoverage(report, scopes) {
 if(!Array.isArray(scopes)||!report.positions)throw Error('SCOPE_AUDIT_INPUT');
 const covered=new Set(scopes.map(s=>s.exchangeId)),uncoveredInventory=[];
 for(const [exchangeId,p]of Object.entries(report.positions)){
  const quantity=[...p.YES,...p.NO].reduce((n,l)=>n+l.quantity,0);
  if(quantity>0&&!covered.has(exchangeId))uncoveredInventory.push({exchangeId,quantity});
 }
 return {uncoveredInventory,terminalLiquidationKnown:report.netLiquidationPnl!==null,completeTerminalExecutionCoverage:uncoveredInventory.length===0&&report.netLiquidationPnl!==null,policyRankingEligible:uncoveredInventory.length===0&&report.netLiquidationPnl!==null&&report.unresolvedOrders===0,limitation:'Final covered scopes and a complete liquidation mark are necessary for ranking; this audit does not prove earlier continuity, queue fills or profitability.'};
}
