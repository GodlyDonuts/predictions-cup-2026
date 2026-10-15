//! Read-only recovery of an exact order ID returned by a durable attempted write.
use baal_transport_service::{Authority,Scope,RequestSpec,Permit,TransportError,ReadOnlyAuthority,Config,Transport,SystemClock,Endpoint,credentials::UnixFileCredentials};
use moloch_live_runtime::dispatch::DispatchLog;
use moloch_replay::{session::{ExclusiveLock,atomic_json,Session},money};
use moloch_live_runtime::feedback;
use sha2::{Digest,Sha256};
use serde_json::{Value,json};
use std::{fs,path::PathBuf,sync::{Arc,Mutex}};
struct RecordedOrder{scope:Scope,id:String,recovery:Arc<Mutex<Option<Value>>>}
impl Authority for RecordedOrder{
 fn can_read(&self,s:&Scope)->bool{s.tournament_id==self.scope.tournament_id&&s.exchanges==self.scope.exchanges}
 fn owns_order(&self,id:&str)->bool{id==self.id}
 fn authorize(&self,s:&Scope,r:&RequestSpec,p:&Permit,b:bool,n:u64)->Result<(),TransportError>{ReadOnlyAuthority.authorize(s,r,p,b,n)}
 fn reconcile_unknown(&self,s:&Scope,b:&Value,_:u64)->Result<(),TransportError>{if self.can_read(s)&&self.recovery.lock().unwrap().as_ref()==Some(b){Ok(())}else{Err(TransportError{code:"RECOVERY_EXACT_PROOF_REQUIRED".into(),status:None,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None})}}
}
fn iso()->String{chrono::DateTime::from(std::time::SystemTime::now()).to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
fn main()->Result<(),Box<dyn std::error::Error>>{
 let args:Vec<_>=std::env::args().collect();if !(args.len()==3||args.len()==4&&args[3]=="--apply"){return Err("usage: moloch-reconcile CONTROL_JSON RECORDED_ORDER_ID [--apply]".into())}let apply=args.len()==4;
 let control:Value=serde_json::from_slice(&fs::read(&args[1])?)?;let root=PathBuf::from(control["root"].as_str().ok_or("RECOVERY_ROOT")?);let lock=PathBuf::from(control["ownerLock"].as_str().ok_or("RECOVERY_LOCK")?);let _lease=ExclusiveLock::acquire(&lock)?;
 let mut log=DispatchLog::open(&root.join("dispatch.ndjson"))?;let mut qid=None;
 for line in fs::read_to_string(root.join("dispatch.ndjson"))?.lines(){let row:Value=serde_json::from_str(line)?;let e=&row["event"];if e["kind"]=="error"&&e["error"]["orderId"]==args[2]{let id=e["qid"].as_u64().ok_or("RECOVERY_QID")?;if !log.attempted.contains(&id)||!log.intents.contains_key(&id){return Err("RECOVERY_NOT_ATTEMPTED".into())}qid=Some(id);}}
 let qid=qid.ok_or("RECOVERY_ORDER_NOT_RECORDED")?;let scope:Scope=serde_json::from_value(control["scope"].clone())?;
 let recovery=Arc::new(Mutex::new(None));let mut transport=Transport::new_with_read_budget(Config{queue_dir:root.join("account-queue"),account_scope:control["accountScope"].as_str().ok_or("RECOVERY_ACCOUNT_SCOPE")?.into(),scope:scope.clone(),maximum_pages:20,writes_enabled:false},Box::new(UnixFileCredentials::approved_file(PathBuf::from(control["credentials"].as_str().ok_or("RECOVERY_CREDENTIAL_FILE")?))?),Box::new(SystemClock),Box::new(RecordedOrder{scope:scope.clone(),id:args[2].clone(),recovery:recovery.clone()}),100,600)?.with_full_account_observation();
 let order=transport.request(Endpoint::OwnedOrder{order_id:args[2].clone()})?;let fills=transport.request(Endpoint::OwnedFills{order_id:args[2].clone()})?;let account=transport.request(Endpoint::Tournament)?;let positions=transport.request(Endpoint::Positions)?;let open=transport.request(Endpoint::Orders{status:"open".into()})?;
 let proof=json!({"kind":"read_only_known_order_reconciliation","qid":qid,"recordedIntent":log.intents[&qid],"order":order,"fills":fills,"account":account,"positions":positions,"openOrders":open,"queue":transport.admission_state()?,"ordersSubmitted":0,"unknownAcceptanceCleared":false});
 let path=root.join(format!("known-order-{}-{}.json",args[2],std::process::id()));if path.exists(){return Err("RECOVERY_OUTPUT_EXISTS".into())}atomic_json(&path,&proof)?;
 if apply{
  if !root.join("EXECUTION_STOP").exists(){return Err("RECOVERY_REQUIRES_STOPPED_WRITER".into())}
  let bank_name=control["bankName"].as_str().ok_or("RECOVERY_BANK")?;if !bank_name.starts_with("bank")||bank_name.contains(['/', '\\']){return Err("RECOVERY_BANK_PATH".into())}
  let seed:Value=serde_json::from_slice(&fs::read(root.join(format!("{bank_name}-account-seed.json")))?)?;let mut bank=Session::open_live(&root.join(bank_name),&control["policy"],&seed)?;
  let quote=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("RECOVERY_NATIVE_INTENT")?;
  let event=feedback::recovered_acknowledgment(&quote,&args[2],&scope.tournament_id,&order,&fills,&iso())?;
  let intent=&log.intents[&qid];let spec:RequestSpec=serde_json::from_value(intent["spec"].clone())?;let key=intent["permit"]["runId"].as_str().ok_or("RECOVERY_RUN")?.to_owned()+":"+intent["permit"]["intentId"].as_str().ok_or("RECOVERY_INTENT")?;
  let binding=json!({"intent":key,"key":spec.body.as_ref().and_then(|v|v["idempotencyKey"].as_str()).ok_or("RECOVERY_KEY")?,"payload_hash":format!("{:x}",Sha256::digest(serde_json::to_vec(&json!({"scope":scope,"spec":spec}))?))});
  if proof["queue"]["state"]["unknown"][&key]!=binding{return Err("RECOVERY_QUEUE_BINDING_MISMATCH".into())}
  if !log.recovered.contains_key(&qid){log.append(json!({"kind":"recovered_ack","qid":qid,"actualOrderId":args[2],"order":order,"fills":fills,"proofPath":path,"event":event}))?;}
  let mut events=vec![event];events.extend(feedback::fills(&quote,&args[2],&scope.tournament_id,&fills,&iso())?);if let Some(e)=feedback::terminal(&quote,&args[2],&scope.tournament_id,&order,&fills,&iso())?{events.push(e)}
  for mut e in events{e["processedAt"]=json!(iso());bank.accept("known-order-recovery",bank.last_source_sequence("known-order-recovery")+1,&e)?;}
  let ex=quote["exchangeId"].as_str().ok_or("RECOVERY_EXCHANGE")?;let expected=bank.engine().wallet.quantity(ex,moloch_replay::Side::Yes) as i128-bank.engine().wallet.quantity(ex,moloch_replay::Side::No) as i128;let rows=positions.data["positions"].as_array().ok_or("RECOVERY_POSITIONS")?;let matching:Vec<_>=rows.iter().filter(|p|p["exchangeId"]==ex).collect();if matching.len()>1||matching.first().map_or(Some(0),|r|r["quantity"].as_i64()).map(i128::from)!=Some(expected){return Err("RECOVERY_POSITION_MISMATCH".into())}
  let expected_cash=baal_transport_service::decimal_units(seed["availableCash"].as_str().ok_or("RECOVERY_SEED_CASH")?)?+bank.engine().wallet.cash-bank.engine().wallet.initial;
  if baal_transport_service::decimal_units(&account.data["myBalance"].to_string())?!=expected_cash{return Err("RECOVERY_CASH_MISMATCH".into())}
  let report=bank.engine().report()?;atomic_json(&root.join(format!("order-{}-reconciled-ledger.json",args[2])),&json!({"proofPath":path,"report":report,"binding":binding,"cash":money(expected_cash),"recoveredAt":iso()}))?;
  *recovery.lock().unwrap()=Some(binding.clone());transport.reconcile_unknown(binding)?;
  if order.data["open"]==false&&!log.closed.contains(&qid){log.append(json!({"kind":"closed","qid":qid,"orderReceiptHash":feedback::digest(&order)?,"fillReceiptHash":feedback::digest(&fills)?}))?;}
  println!("{}",json!({"applied":true,"qid":qid,"actualOrderId":args[2],"orderOpen":order.data["open"],"report":report,"queueUnknown":transport.admission_state()?["state"]["unknown"],"ordersSubmitted":0}));
 }else{println!("{}",json!({"proof":path,"order":proof["order"],"fills":proof["fills"],"openOrderCount":proof["openOrders"]["data"]["data"].as_array().map(Vec::len),"queueUnknown":proof["queue"]["state"]["unknown"]}));}Ok(())
}
