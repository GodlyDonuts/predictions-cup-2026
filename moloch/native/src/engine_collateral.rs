//! Actual ALL continuation ledger. Primary saved OpenAPI SHA256
//! 6b5ff9a661c06f1a2d1dda131bf870a70f4f79699ac9937375a278861e1f81bf:
//! AllExecutionEconomics, PortfolioCollateral, TournamentSettlementEntry.
//!
//! `actual_collateral_{baseline,execution,settlement}` are owner-admitted events,
//! never deserialized account instructions. The sole runtime MUST verify original
//! scoped receipts, original dispatch/queue heads and its exclusive lease before
//! constructing boundary. Hashes bind that admitted evidence, not authenticate it.
//! Session independently pins actual bank/run/head. No snapshot names a sequence
//! barrier in the official schema; sequential receipts alone prove no atomicity.
//!
//! Each event includes eventId, bankScope(runId,bankManifestHash,tournamentId,
//! tournamentSlug,profileId), expectedReportHash, boundary(nativeHead,
//! ordinaryDispatchHash,packageDispatchHash,queueHeadHash,ownerLeaseHash,
//! unknownCount=0,activeRequest=null,unsubmittedOrderIds), and after{account,
//! collateral}, both original serialized Receipts. Continuations also include
//! before{account,collateral}. Execution `receipts` contains original POST ACK
//! receipts covering ALL newly admitted native fills (ACKs never create fills).
//! Settlement has original complete scoped `settlements` receipt plus explicit
//! `settlementIds`; nullable/ambiguous attribution and corrections fail closed.
//!
//! Borrowed principal stays OUTSIDE Wallet sizing cash and realized profit.
//! Official cash, debt, available advance and component floors remain separate.
use super::*;
#[derive(Clone)]
struct Snapshot{cash:i128,power:i128,available:i128,debt:i128,components:Value,start:i64,end:i64}
#[derive(Clone)]
pub(super) struct Ledger{scope:Value,snapshot:Snapshot,wallet_cash:i128,opening_cash:i128,opening_wallet:i128,drawn:i128,repaid:i128,settled:i128,fill_count:usize,events:BTreeMap<String,String>,orders:BTreeSet<String>,settlements:BTreeMap<String,String>}
fn need(ok:bool,why:&'static str)->Result<()>{if ok{Ok(())}else{Err(why)}}
fn money_value(v:&Value)->Result<i128>{let n=if let Some(s)=v.as_str(){amount(s)?}else{amount(&v.to_string())?};need(n>=0,"COLLATERAL_NEGATIVE")?;Ok(n)}
fn signed_money(v:&Value)->Result<i128>{if let Some(s)=v.as_str(){amount(s)}else{amount(&v.to_string())}}
fn add(a:i128,b:i128)->Result<i128>{a.checked_add(b).ok_or("COLLATERAL_RANGE")}
fn uuid(v:&Value)->bool{v.as_str().is_some_and(|s|s.len()==36&&s.bytes().enumerate().all(|(i,b)|if [8,13,18,23].contains(&i){b==b'-'}else{b.is_ascii_hexdigit()}))}
fn receipt(v:&Value,endpoint:&str,at:i64)->Result<(i64,i64)>{need(v["status"]==200&&v["endpoint"]==endpoint,"COLLATERAL_RECEIPT_SCOPE")?;let start=ms(&v["requestStartedAt"])?;let end=ms(&v["receivedAt"])?;need(start>=0&&start<=end&&end<=at,"COLLATERAL_RECEIPT_CLOCK")?;Ok((start,end))}
fn snapshot(v:&Value,s:&Value,at:i64)->Result<Snapshot>{
 let tid=s["tournamentId"].as_str().ok_or("COLLATERAL_SCOPE")?;let a=&v["account"];let c=&v["collateral"];
 let(sa,ea)=receipt(a,&format!("/account?tournamentId={tid}"),at)?;let(sc,ec)=receipt(c,&format!("/portfolio/collateral?tournamentId={tid}"),at)?;
 need((ea<=sc||ec<=sa)&&ea.max(ec)-sa.min(sc)<=20_000,"COLLATERAL_SNAPSHOT_SPAN")?;
 need(a["data"]["id"]==s["profileId"],"COLLATERAL_PROFILE")?;
 let cash=money_value(&a["data"]["balance"])?;let available=money_value(&a["data"]["availableAdvance"])?;let power=money_value(&a["data"]["buyingPower"])?;
 need(power==add(cash,available)?&&available==money_value(&c["data"]["availableAdvance"])?,"COLLATERAL_BUYING_POWER")?;
 let rows=c["data"]["data"].as_array().filter(|r|r.len()<=4096).ok_or("COLLATERAL_COMPONENTS")?;
 let mut components=serde_json::Map::new();let mut debt=0;
 for r in rows{need(r["profileId"]==s["profileId"]&&r["tournamentId"]==s["tournamentId"],"COLLATERAL_COMPONENT_SCOPE")?;let key=r["componentId"].as_str().filter(|s|!s.is_empty()&&s.len()<=512).ok_or("COLLATERAL_COMPONENT_ID")?;
  let d=money_value(&r["outstandingAdvance"])?;debt=add(debt,d)?;if !r["guaranteedPayoutFloor"].is_null(){money_value(&r["guaranteedPayoutFloor"])?;}
  for key in ["coveredExchangeIds","relationshipIds"]{let xs=r[key].as_array().ok_or("COLLATERAL_COMPONENT_MEMBERS")?;need(xs.len()<=4096,"COLLATERAL_COMPONENT_MEMBERS")?;let mut seen=BTreeSet::new();for x in xs{let val=if key=="relationshipIds"{need(uuid(x),"COLLATERAL_RELATIONSHIP")?;x.to_string()}else{let n=x.as_u64().filter(|n|*n>0).ok_or("COLLATERAL_EXCHANGE")?;n.to_string()};need(seen.insert(val),"COLLATERAL_DUPLICATE_MEMBER")?;}}
  need(components.insert(key.into(),r.clone()).is_none(),"COLLATERAL_DUPLICATE_COMPONENT")?;
 }
 Ok(Snapshot{cash,power,available,debt,components:components.into(),start:sa.min(sc),end:ea.max(ec)})
}
impl Engine{
 pub fn collateral_report(&self)->Result<Value>{let l=self.collateral.as_ref().ok_or("COLLATERAL_NOT_INITIALIZED")?;let cash=add(l.snapshot.cash,self.wallet.cash-l.wallet_cash)?;let adjustment=cash-l.opening_cash-(self.wallet.cash-l.opening_wallet);
  Ok(json!({"version":1,"officialCash":money(cash),"observedOfficialCash":money(l.snapshot.cash),"pendingNativeFillEconomics":self.fills.len()!=l.fill_count||self.wallet.cash!=l.wallet_cash,"officialDebt":money(l.snapshot.debt),"netCashAfterDebt":money(cash-l.snapshot.debt),"collateralCashAdjustment":money(adjustment),"availableAdvanceObserved":money(l.snapshot.available),"availableAdvanceAdmittedForSizing":"0.000000","cumulativeBorrowed":money(l.drawn),"cumulativeRepaid":money(l.repaid),"settlementPayout":money(l.settled),"componentStates":l.snapshot.components,"events":l.events.len(),"actualSettlements":l.settlements.len(),"snapshotAtMs":l.snapshot.end,"cashViewIncludesLaterNativeFills":true,"officialAccountProfit":Value::Null}))
 }
 pub(super) fn audit_collateral(&self)->Result<()>{if let Some(l)=&self.collateral{need(l.snapshot.debt==l.drawn-l.repaid&&l.snapshot.debt>=0&&l.snapshot.cash-l.opening_cash-(l.wallet_cash-l.opening_wallet)==l.snapshot.debt&&l.fill_count<=self.fills.len(),"COLLATERAL_ACCOUNTING_IDENTITY")?;}Ok(())}
 pub(super) fn apply_collateral(&mut self,e:&Value,at:i64)->Result<()>{
  need(self.live_mode&&e["source"]=="official_sig_api_v1","COLLATERAL_SOURCE")?;
  let scope=&e["bankScope"];need(uuid(&scope["tournamentId"])&&uuid(&scope["profileId"])&&scope["runId"].as_str().is_some_and(|s|!s.is_empty())&&scope["tournamentSlug"].as_str().is_some_and(|s|!s.is_empty()),"COLLATERAL_SCOPE")?;
  need(self.books.values().all(|b|b.tournament==scope["tournamentId"].as_str().unwrap())&&self.orders.iter().all(|q|q.tournament==scope["tournamentId"].as_str().unwrap()),"COLLATERAL_NATIVE_TOURNAMENT")?;
  let event_id=e["eventId"].as_str().filter(|s|!s.is_empty()&&s.len()<=512).ok_or("COLLATERAL_EVENT_ID")?;
  let mut semantic=e.clone();semantic.as_object_mut().ok_or("COLLATERAL_EVENT")?.remove("sourceRef");let digest=hash(&semantic);
  if let Some(old)=self.collateral.as_ref().and_then(|l|l.events.get(event_id)){return need(old==&digest,"COLLATERAL_EVENT_CONFLICT")}
  need(e["expectedReportHash"]==hash(&self.report()?),"COLLATERAL_STALE_NATIVE_STATE")?;
  let b=&e["boundary"];need(b["unknownCount"]==0&&b.get("activeRequest")==Some(&Value::Null),"COLLATERAL_PHYSICAL_UNCERTAINTY")?;
  for k in ["ordinaryDispatchHash","packageDispatchHash","queueHeadHash","ownerLeaseHash"]{let s=b[k].as_str().ok_or("COLLATERAL_BOUNDARY")?;need(s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit()),"COLLATERAL_BOUNDARY")?;}
  let unsubmitted:BTreeSet<u64>=b["unsubmittedOrderIds"].as_array().ok_or("COLLATERAL_UNSUBMITTED_PROOF")?.iter().map(|v|v.as_u64().ok_or("COLLATERAL_UNSUBMITTED_PROOF")).collect::<Result<_>>()?;
  let pending:BTreeSet<u64>=self.orders.iter().filter(|q|!q.retired).map(|q|q.id).collect();need(unsubmitted==pending&&pending.iter().all(|id|!self.live_orders.contains_key(id)&&!self.is_package_order(*id)),"COLLATERAL_PENDING_PHYSICAL_ORDER")?;
  need(!self.package_blocks_submissions(),"COLLATERAL_UNRESOLVED_PACKAGE")?;
  let after=snapshot(&e["after"],scope,at)?;need(at-after.end<=20_000,"COLLATERAL_AFTER_STALE")?;
  if e["type"]=="actual_collateral_baseline"{
   need(self.collateral.is_none()&&after.debt==0,"COLLATERAL_BASELINE_DEBT_OR_REPEAT")?;
   self.collateral=Some(Ledger{scope:scope.clone(),snapshot:after,wallet_cash:self.wallet.cash,opening_cash:money_value(&e["after"]["account"]["data"]["balance"])?,opening_wallet:self.wallet.cash,drawn:0,repaid:0,settled:0,fill_count:self.fills.len(),events:[(event_id.into(),digest)].into(),orders:BTreeSet::new(),settlements:BTreeMap::new()});return Ok(())
  }
  let mut l=self.collateral.clone().ok_or("COLLATERAL_BASELINE_REQUIRED")?;need(l.scope==*scope,"COLLATERAL_SCOPE_CHANGED")?;
  let before=snapshot(&e["before"],scope,at)?;
  need(before.cash==l.snapshot.cash&&before.debt==l.snapshot.debt&&before.power==l.snapshot.power&&before.components==l.snapshot.components&&before.start>=l.snapshot.start&&before.end>=l.snapshot.end&&before.end<=after.start&&after.end-before.start<=120_000,"COLLATERAL_WINDOW_OR_BEFORE")?;
  for(key,old)in before.components.as_object().ok_or("COLLATERAL_COMPONENTS")?{if let Some(new)=after.components.get(key){for field in ["profileId","tournamentId","coveredExchangeIds","relationshipIds"]{need(old[field]==new[field],"COLLATERAL_COMPONENT_IDENTITY_CHANGED")?;}}}
  let(mut drawn,mut repaid)=(0,0);
  if e["type"]=="actual_collateral_execution"{
   let new=&self.fills[l.fill_count..];need(!new.is_empty()&&new.iter().all(|f|f["counterfactual"]==false),"COLLATERAL_ACTUAL_FILLS_REQUIRED")?;
   let ids:BTreeSet<String>=new.iter().map(|f|f["actualOrderId"].as_str().map(str::to_owned).ok_or("COLLATERAL_FILL_ID")).collect::<Result<_>>()?;
   let mut raws=BTreeMap::new();for r in e["receipts"].as_array().filter(|r|r.len()<=16).ok_or("COLLATERAL_EXECUTION_RECEIPTS")?{
    let endpoint=r["endpoint"].as_str().ok_or("COLLATERAL_EXECUTION_SCOPE")?;need(["/orders","/orders/multi-leg"].contains(&endpoint),"COLLATERAL_EXECUTION_SCOPE")?;let(start,end)=receipt(r,endpoint,at)?;need(start>=before.end&&end<=after.start,"COLLATERAL_EXECUTION_CLOCK")?;
    let rows:Vec<&Value>=if endpoint=="/orders"{vec![&r["data"]]}else{r["data"]["results"].as_array().ok_or("COLLATERAL_ACK_RESULTS")?.iter().map(|v|&v["data"]).collect()};
    for raw in rows{let id=raw["orderId"].as_str().map(str::to_owned).or_else(||raw["orderId"].as_u64().map(|n|n.to_string())).ok_or("COLLATERAL_ORDER_ID")?;need(raws.insert(id,(raw.clone(),start,end)).is_none(),"COLLATERAL_DUPLICATE_ORDER")?;}
   }
   need(raws.keys().cloned().collect::<BTreeSet<_>>()==ids,"COLLATERAL_ORDER_COVERAGE")?;let(mut trade_cash,mut impact)=(0,0);
   for(id,(raw,start,end))in raws{need(l.orders.insert(id.clone()),"COLLATERAL_ORDER_ALREADY_ACCOUNTED")?;let qid=*self.live_orders.iter().find(|(_,v)|**v==id).ok_or("COLLATERAL_UNOWNED_ORDER")?.0;let q=self.orders.iter().find(|q|q.id==qid).ok_or("COLLATERAL_UNOWNED_ORDER")?;
    need(q.retired&&q.filled==q.quantity&&raw["exchangeId"]==q.exchange&&raw["action"]==if q.buy{"buy"}else{"sell"}&&raw["side"]==if q.side==Side::Yes{"yes"}else{"no"}&&raw["quantity"].as_u64()==Some(q.quantity),"COLLATERAL_ORDER_BINDING")?;
    let(mut quantity,mut cost,mut fees)=(0u64,0,0);for f in new.iter().filter(|f|f["actualOrderId"]==id){let n=f["quantity"].as_u64().ok_or("COLLATERAL_FILL_QUANTITY")?;quantity=quantity.checked_add(n).ok_or("COLLATERAL_RANGE")?;cost=add(cost,dec(&f["price"])?.checked_mul(n as i128).ok_or("COLLATERAL_RANGE")?)?;fees=add(fees,dec(&f["fee"])?)?;let executed=ms(&f["executedAt"])?;need(executed>=start&&executed<=end,"COLLATERAL_FILL_WINDOW")?;}
    let a=&raw["all"];need(quantity==q.quantity&&cost==money_value(&a["fullNotionalCost"])?&&cost==money_value(&raw["totalCost"])?,"COLLATERAL_ACTUAL_NOTIONAL")?;
    let own_floor=money_value(&a["guaranteedPayoutFloorAfter"])?;let own_debt=money_value(&a["outstandingAdvanceAfter"])?;let relationships=a["relationshipIds"].as_array().ok_or("COLLATERAL_ACK_RELATIONSHIPS")?;let mut seen_relationships=BTreeSet::new();for relation in relationships{need(uuid(relation)&&seen_relationships.insert(relation.to_string()),"COLLATERAL_ACK_RELATIONSHIPS")?;}
    let d=money_value(&a["collateralSavings"])?;let p=money_value(&a["collateralRepayment"])?;let redemption=money_value(&a["redemptionCredit"])?;need(money_value(&a["effectiveEntryCost"])?==if q.buy{cost-d}else{cost}&&d<=cost&&(q.buy||d==0),"COLLATERAL_ENTRY_COST")?;
    drawn=add(drawn,d)?;repaid=add(repaid,p)?;impact=add(impact,signed_money(&a["netBuyingPowerImpact"])?)?;
    trade_cash=add(trade_cash,add(if q.buy{-cost}else{cost},redemption-fees)?)?;
    if a["componentId"].is_null(){need(own_floor==0&&own_debt==0&&relationships.is_empty(),"COLLATERAL_UNLINKED_AFTER")?;}else{let k=a["componentId"].as_str().ok_or("COLLATERAL_COMPONENT_ID")?;let component=after.components.get(k).ok_or("COLLATERAL_OWN_COMPONENT_AFTER")?;need(component["coveredExchangeIds"].as_array().is_some_and(|ids|ids.iter().any(|id|id.as_u64().map(|n|n.to_string()).as_deref()==Some(q.exchange.as_str()))),"COLLATERAL_OWN_COMPONENT_MEMBERSHIP")?;need(money_value(&component["outstandingAdvance"])?==money_value(&a["outstandingAdvanceAfter"])?&&!component["guaranteedPayoutFloor"].is_null()&&money_value(&component["guaranteedPayoutFloor"])?==money_value(&a["guaranteedPayoutFloorAfter"])?,"COLLATERAL_OWN_COMPONENT_AFTER")?;}
   }
   need(self.wallet.cash-l.wallet_cash==trade_cash&&before.power-after.power==impact,"COLLATERAL_NATIVE_CASH_OR_POWER")?;
  }else if e["type"]=="actual_collateral_settlement"{
   need(self.fills.len()==l.fill_count&&self.wallet.cash==l.wallet_cash,"COLLATERAL_UNACCOUNTED_FILLS")?;
   let r=&e["settlements"];let endpoint=format!("/tournaments/{}/portfolio/settlements?limit=200",scope["tournamentSlug"].as_str().ok_or("COLLATERAL_SLUG")?);let(start,end)=receipt(r,&endpoint,at)?;
   need(start>=before.end&&end<=after.start&&r["data"]["coverage"]["complete"]==true&&r["data"]["pagination"]["hasMore"]==false&&r["data"]["pagination"]["nextCursor"].is_null(),"COLLATERAL_SETTLEMENT_COVERAGE")?;
   let selected=e["settlementIds"].as_array().filter(|a|!a.is_empty()&&a.len()<=200).ok_or("COLLATERAL_SETTLEMENT_IDS")?;let mut chosen=BTreeSet::new();
   for selected in selected{let id=selected.as_str().ok_or("COLLATERAL_SETTLEMENT_ID")?;need(chosen.insert(id),"COLLATERAL_SETTLEMENT_DUPLICATE")?;let rows:Vec<_>=r["data"]["data"].as_array().ok_or("COLLATERAL_SETTLEMENT_ROWS")?.iter().filter(|v|v["id"]==id).collect();need(rows.len()==1&&!l.settlements.contains_key(id),"COLLATERAL_SETTLEMENT_ALREADY_ACCOUNTED")?;let row=rows[0];
    need(["win","loss","refund"].iter().any(|s|row["type"]==*s),"COLLATERAL_SETTLEMENT_KIND")?;let ex=id_value(&row["exchangeId"])?;let market=id_value(&row["marketId"])?;let side=match row["settlementOption"].as_str(){Some("YES")=>Side::Yes,Some("NO")=>Side::No,_=>return Err("COLLATERAL_SETTLEMENT_SIDE")};
    let quantity=money_value(&row["shares"])?;need(quantity>0&&quantity%SCALE==0,"COLLATERAL_SETTLEMENT_QUANTITY")?;let n=u64::try_from(quantity/SCALE).map_err(|_|"COLLATERAL_SETTLEMENT_QUANTITY")?;let payout=money_value(&row["payout"])?;need(payout<=quantity&&payout%(n as i128)==0&&self.wallet.quantity(&ex,side)==n&&self.wallet.quantity(&ex,side.other())==0&&(self.orders.iter().any(|q|q.exchange==ex&&q.market==market)||self.inherited.as_ref().is_some_and(|i|i.source_positions.as_array().is_some_and(|rows|rows.iter().any(|p|p["exchangeId"]==ex&&p["marketId"]==market)))),"COLLATERAL_SETTLEMENT_ATTRIBUTION")?;need(!self.package_protects_exchange(&ex),"COLLATERAL_PACKAGE_INVENTORY_PROTECTED")?;let created=ms(&row["createdAt"])?;need(created>=before.end&&created<=end,"COLLATERAL_SETTLEMENT_CLOCK")?;
    self.wallet.settle_owned_exact(&ex,side,n,payout)?;self.consume_inherited(&ex,side,n,payout/n as i128)?;l.settled=add(l.settled,payout)?;l.settlements.insert(id.into(),hash(row));
   }
   need(after.debt<=before.debt,"COLLATERAL_SETTLEMENT_NEW_BORROW")?;repaid=before.debt-after.debt;
  }else{return Err("COLLATERAL_EVENT_TYPE")}
  need(after.debt-before.debt==drawn-repaid&&after.cash-before.cash==self.wallet.cash-l.wallet_cash+drawn-repaid,"COLLATERAL_CASH_DEBT_EQUATION")?;
  l.drawn=add(l.drawn,drawn)?;l.repaid=add(l.repaid,repaid)?;l.snapshot=after;l.wallet_cash=self.wallet.cash;l.fill_count=self.fills.len();l.events.insert(event_id.into(),digest);self.collateral=Some(l);Ok(())
 }
}
fn id_value(v:&Value)->Result<String>{id(v)}
