//! Attribution of human-approved, already-existing account cash to this bank.
//! Not a deposit, borrow, fill, settlement or profit. Original manifest seed and
//! original policy.initialCash remain immutable; effective Wallet.initial and
//! cash rise together. Original receipt hashes bind integrity, not authority.
use super::*;
const MAX_EXTRA:i128=55_579_915_000;
const MAX_EFFECTIVE:i128=95_579_915_000;
fn need(ok:bool,why:&'static str)->Result<()>{if ok{Ok(())}else{Err(why)}}
fn sha(v:&Value)->Result<&str>{v.as_str().filter(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))).ok_or("CAPITAL_HASH")}
fn monetary(v:&Value)->Result<i128>{let n=if let Some(s)=v.as_str(){amount(s)?}else{amount(&v.to_string())?};need(n>=0,"CAPITAL_NEGATIVE")?;Ok(n)}
fn original_digest(v:&Value)->Result<String>{
 #[derive(serde::Serialize)]#[serde(rename_all="camelCase")]struct Receipt<'a>{endpoint:&'a Value,request_started_at:&'a Value,received_at:&'a Value,status:&'a Value,data:&'a Value}
 Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(&Receipt{endpoint:&v["endpoint"],request_started_at:&v["requestStartedAt"],received_at:&v["receivedAt"],status:&v["status"],data:&v["data"]}).map_err(|_|"CAPITAL_RECEIPT")?)))
}
fn receipt(v:&Value,endpoint:&str,at:i64)->Result<(i64,i64)>{
 need(v["endpoint"]==endpoint&&v["status"]==200,"CAPITAL_RECEIPT_SCOPE")?;let start=ms(&v["requestStartedAt"])?;let end=ms(&v["receivedAt"])?;
 need(start>=0&&start<=end&&end<=at&&at-start<=20_000,"CAPITAL_RECEIPT_CLOCK")?;Ok((start,end))
}
impl Engine{
 pub fn owner_capital_allocation_record(&self)->Option<Value>{self.capital_allocation.clone()}
 pub(super)fn audit_owner_capital(&self)->Result<()>{if let Some(r)=&self.capital_allocation{
  let net=dec(&r["netContributions"])?;
  need(net>0&&net<=MAX_EXTRA&&self.policy.initial==40_000*SCALE&&self.wallet.initial==self.policy.initial+net&&r["effectiveInitialCash"]==money(self.wallet.initial)&&r["effectiveMaximumCommitted"]==money(self.policy.committed)&&self.wallet.maximum_committed==self.policy.committed,"CAPITAL_ACCOUNTING_IDENTITY")?;
 }Ok(())}
 pub(super)fn apply_owner_capital(&mut self,e:&Value,at:i64)->Result<()>{
  need(self.live_mode&&e["source"]=="moloch_live_dispatcher"&&e["authorizationId"]=="human_moloch_additional_cash_20261016","CAPITAL_AUTHORITY")?;
  need(ms(&e["processedAt"])?==at&&ms(&e["receivedAt"])?<=at,"CAPITAL_EVENT_CLOCK")?;
  need(self.collateral.is_none(),"CAPITAL_COLLATERAL_LEDGER_UNSUPPORTED")?;
  let s=&e["bankScope"];let tid=s["tournamentId"].as_str().filter(|s|!s.is_empty()).ok_or("CAPITAL_SCOPE")?;let slug=s["tournamentSlug"].as_str().filter(|s|!s.is_empty()).ok_or("CAPITAL_SCOPE")?;
  need(s["runId"].as_str().is_some_and(|s|!s.is_empty())&&s["profileId"].as_str().is_some_and(|s|!s.is_empty()),"CAPITAL_SCOPE")?;sha(&s["bankManifestHash"])?;sha(&e["controlSha256"])?;sha(&e["mandateSha256"])?;
  need(e["expectedReportHash"]==hash(&self.report()?)&&e["expectedPolicySha256"]==hash(&self.policy.raw),"CAPITAL_STALE_NATIVE_STATE")?;
  need(self.books.values().all(|b|b.tournament==tid)&&self.orders.iter().all(|q|q.tournament==tid),"CAPITAL_NATIVE_TOURNAMENT")?;
  let prior=self.capital_allocation.as_ref().map_or(Value::Null,|r|r["sha256"].clone());need(e.get("expectedPreviousAllocationSha256")==Some(&prior),"CAPITAL_PREVIOUS_RECORD")?;
  let allocation=e["allocationId"].as_str().filter(|s|!s.is_empty()&&s.len()<=128).ok_or("CAPITAL_ALLOCATION_ID")?;
  let mut contributions=self.capital_allocation.as_ref().map_or(Ok(Vec::new()),|r|r["contributions"].as_array().cloned().ok_or("CAPITAL_RECORD"))?;
  need(contributions.len()<16&&!contributions.iter().any(|r|r["allocationId"]==allocation),"CAPITAL_DUPLICATE_OR_BOUND")?;
  if let Some(r)=&self.capital_allocation{need(r["bankScope"]==*s,"CAPITAL_SCOPE_CHANGED")?;}
  let delta=dec(&e["delta"])?;let net=self.capital_allocation.as_ref().map_or(Ok(0),|r|dec(&r["netContributions"]))?.checked_add(delta).ok_or("CAPITAL_RANGE")?;
  need(delta>0&&net<=MAX_EXTRA&&self.policy.initial==40_000*SCALE&&self.policy.market==10_000*SCALE&&self.policy.quantity<=10000,"CAPITAL_MANDATE_CAP")?;
  let effective=self.wallet.initial.checked_add(delta).ok_or("CAPITAL_RANGE")?;let committed=self.policy.committed.checked_add(delta).ok_or("CAPITAL_RANGE")?;
  need(e["expectedInitialCash"]==money(self.wallet.initial)&&e["expectedMaximumCommitted"]==money(self.policy.committed)&&e["effectiveInitialCash"]==money(effective)&&e["effectiveMaximumCommitted"]==money(committed)&&effective<=MAX_EFFECTIVE&&committed<=MAX_EFFECTIVE,"CAPITAL_EFFECTIVE_CAP")?;
  let b=&e["boundary"];need(b["unknownCount"]==0&&b.get("activeRequest")==Some(&Value::Null),"CAPITAL_PHYSICAL_UNCERTAINTY")?;
  for k in ["ordinaryDispatchHash","packageDispatchHash","queueHeadHash","ownerLeaseHash"]{sha(&b[k])?;}
  let pending:BTreeSet<u64>=self.orders.iter().filter(|q|!q.retired).map(|q|q.id).collect();let stated=b["unsubmittedOrderIds"].as_array().ok_or("CAPITAL_UNSUBMITTED")?;
  let unsubmitted:BTreeSet<u64>=stated.iter().map(|v|v.as_u64().ok_or("CAPITAL_UNSUBMITTED")).collect::<Result<_>>()?;
  need(stated.len()==unsubmitted.len()&&pending==unsubmitted&&pending.iter().all(|q|!self.live_orders.contains_key(q)&&!self.is_package_order(*q))&&!self.package_blocks_submissions(),"CAPITAL_PENDING_PHYSICAL_ORDER")?;
  let scope=e["scopeExchanges"].as_object().filter(|m|!m.is_empty()&&m.len()<=4096).ok_or("CAPITAL_EXCHANGE_SCOPE")?;for(ex,market)in scope{id(&json!(ex))?;id(market)?;}
  for q in &self.orders{need(scope.get(&q.exchange)==Some(&json!(q.market)),"CAPITAL_EXCHANGE_SCOPE")?;}
  for book in self.books.values(){need(scope.get(&book.exchange)==Some(&json!(book.market)),"CAPITAL_EXCHANGE_SCOPE")?;}
  let rs=&e["receipts"];let endpoints=[("account",format!("/account?tournamentId={tid}")),("collateral",format!("/portfolio/collateral?tournamentId={tid}")),("positions",format!("/tournaments/{slug}/portfolio/positions")),("orders",format!("/orders?status=open&limit=200&tournamentId={tid}"))];
  let evidence=e["originalSourceEvidence"].as_array().filter(|v|v.len()==4).ok_or("CAPITAL_ORIGINAL_SOURCE")?;let mut clocks=Vec::new();
  for((name,path),proof)in endpoints.iter().zip(evidence){clocks.push(receipt(&rs[*name],path,at)?);need(proof["receiptSha256"]==original_digest(&rs[*name])?,"CAPITAL_ORIGINAL_SOURCE")?;sha(&proof["retainedBytesSha256"])?;}
  clocks.sort();need(clocks.windows(2).all(|w|w[0].1<=w[1].0)&&clocks.last().unwrap().1-clocks[0].0<=20_000,"CAPITAL_RECEIPT_OVERLAP")?;
  let a=&rs["account"]["data"];let c=&rs["collateral"]["data"];
  need(a["id"]==s["profileId"]&&monetary(&a["availableAdvance"])?==0&&monetary(&c["availableAdvance"])?==0&&monetary(&a["balance"])?==monetary(&a["buyingPower"])?&&self.wallet.cash.checked_add(delta).is_some_and(|cash|cash<=monetary(&a["balance"]).unwrap_or(-1)),"CAPITAL_CASH_OR_ADVANCE")?;
  let components=c["data"].as_array().filter(|v|v.len()<=4096).ok_or("CAPITAL_COMPONENTS")?;let mut component_ids=BTreeSet::new();for row in components{need(row["profileId"]==s["profileId"]&&row["tournamentId"]==tid&&row["componentId"].as_str().is_some_and(|id|!id.is_empty()&&component_ids.insert(id.to_owned()))&&monetary(&row["outstandingAdvance"])?==0,"CAPITAL_DEBT_OR_COMPONENT_SCOPE")?;}
  let orders=&rs["orders"]["data"];need(orders["coverage"]["complete"]==true&&orders["pagination"]["hasMore"]==false&&orders["pagination"].get("nextCursor")==Some(&Value::Null)&&orders["data"].as_array().is_some_and(Vec::is_empty),"CAPITAL_OPEN_ORDERS_OR_COVERAGE")?;
  let positions=&rs["positions"]["data"];need(positions.get("pagination").is_none(),"CAPITAL_POSITION_PAGINATION")?;let rows=positions["positions"].as_array().filter(|a|a.len()<=4096).ok_or("CAPITAL_POSITIONS")?;let mut seen=BTreeSet::new();
  for row in rows{let ex=id(&row["exchangeId"])?;need(seen.insert(ex.clone())&&scope.get(&ex)==Some(&row["marketId"]),"CAPITAL_POSITION_SCOPE")?;let quantity=row["quantity"].as_i64().ok_or("CAPITAL_POSITION_QUANTITY")?;let yes=self.wallet.quantity(&ex,Side::Yes);let no=self.wallet.quantity(&ex,Side::No);need(yes.min(no)==0&&quantity as i128==yes as i128-no as i128,"CAPITAL_POSITION_MISMATCH")?;}
  need(self.wallet.lots.iter().all(|((ex,_),lots)|lots.iter().all(|l|l.quantity==0)||seen.contains(ex)),"CAPITAL_POSITION_MISSING")?;
  let mut raw=self.policy.raw.clone();raw["maximumCommitted"]=json!(money(committed));let policy=Policy::new(&raw)?;
  contributions.push(json!({"allocationId":allocation,"delta":money(delta),"controlSha256":e["controlSha256"],"mandateSha256":e["mandateSha256"],"originalSourceEvidence":evidence,"boundary":b,"at":e["processedAt"],"previousAllocationSha256":prior}));
  let mut record=json!({"schemaVersion":1,"bankScope":s,"originalInitialCash":money(self.policy.initial),"netContributions":money(net),"effectiveInitialCash":money(effective),"effectiveMaximumCommitted":money(committed),"contributions":contributions});record["sha256"]=json!(hash(&record));
  self.wallet.allocate_owner_cash_exact(delta,committed)?;self.policy=policy;self.capital_allocation=Some(record);Ok(())
 }
}
