use moloch_live_runtime::{feedback::Result,reconciliation::{Backend,Reconciler,owned_from_quote},reconciliation_plan::OwnedOrder};
use baal_transport_service::{Endpoint,Receipt,Scope,TransportError};
use serde_json::{Value,json};
use std::collections::{BTreeMap,BTreeSet};
const T:&str="bda92870-621e-47b0-bc3c-3602c5c26f55";
fn base()->u64{chrono::DateTime::parse_from_rfc3339("2026-10-15T10:00:00Z").unwrap().timestamp_millis() as u64}
fn iso(n:u64)->String{chrono::DateTime::from_timestamp_millis(n as i64).unwrap().to_rfc3339()}
fn scope()->Scope{Scope{tournament_id:T.into(),tournament_slug:"midterm-elections".into(),exchanges:[("101".into(),"201".into())].into()}}
struct Fake{
 now:u64,gap:u64,quotes:BTreeMap<u64,Value>,reads:Vec<String>,seen:BTreeSet<String>,closed:BTreeSet<u64>,
 server_fills:BTreeMap<String,u64>,fill_on_read:BTreeMap<String,u64>,server_closed:BTreeSet<String>,
 first_fill_queue_wait:u64,bad_first_fill:bool,omit_fill_limit:bool,aggregate_bad:bool,
 foreign:bool,fail_apply:bool,aggregate_error:Option<&'static str>,applied:usize,
}
impl Fake{
 fn new(n:u64)->Self{Self{now:base()+1000,gap:600,quotes:(1..=n).map(|id|(id,json!({"id":id,"marketId":"201","exchangeId":"101","side":"YES","action":"buy","quantity":10,"filled":0,"price":"0.450000","createdAt":base(),"expiresAt":base()+3_600_000,"retired":false}))).collect(),reads:vec![],seen:BTreeSet::new(),closed:BTreeSet::new(),server_fills:BTreeMap::new(),fill_on_read:BTreeMap::new(),server_closed:BTreeSet::new(),first_fill_queue_wait:0,bad_first_fill:false,omit_fill_limit:false,aggregate_bad:false,foreign:false,fail_apply:false,aggregate_error:None,applied:0}}
 fn pending(&self)->Vec<OwnedOrder>{self.quotes.iter().filter(|(id,_)|!self.closed.contains(id)).map(|(id,q)|owned_from_quote(q,&(1000+id).to_string(),false,false).unwrap()).collect()}
 fn qid(actual:&str)->u64{actual.parse::<u64>().unwrap()-1000}
 fn row(&self,id:u64,open:bool)->Value{
  let q=&self.quotes[&id];let actual=(1000+id).to_string();let filled=self.server_fills.get(&actual).copied().unwrap_or(0);
  json!({"id":actual,"marketId":"201","exchangeId":"101","tournamentId":T,"side":"yes","action":"buy","priceLimit":0.45,"quantity":if open{10-filled}else{10},"quantityFilled":if open{Value::Null}else{json!(filled)},"open":open,"terminalReasonCode":Value::Null,"createdAt":iso(base()),"expirationDate":iso(q["expiresAt"].as_u64().unwrap())})
 }
}
impl Backend for Fake{
 fn now_ms(&self)->u64{self.now}
 fn read(&mut self,ep:Endpoint)->Result<Receipt>{
  if matches!(ep,Endpoint::OwnedFills{..})&&self.first_fill_queue_wait>0{self.now+=self.first_fill_queue_wait;self.first_fill_queue_wait=0;}
  let start=self.now;self.now+=self.gap;
  let (path,data)=match ep{
   Endpoint::Orders{status}=>{
    assert_eq!(status,"open");self.reads.push("aggregate".into());
    if let Some(code)=self.aggregate_error{return Err(TransportError{code:code.into(),status:None,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None}.into())}
    let mut rows:Vec<_>=self.quotes.keys().filter(|id|{let actual=(1000+**id).to_string();!self.server_closed.contains(&actual)&&self.server_fills.get(&actual).copied().unwrap_or(0)<10}).map(|id|self.row(*id,true)).collect();
    if self.foreign{let mut row=self.row(1,true);row["id"]=json!("99999");rows.push(row);}
    (format!("/orders?status=open&limit=200&tournamentId={T}"),json!({"data":rows,"coverage":{"complete":true,"projectedThroughSequence":if self.aggregate_bad{Value::Null}else{json!(start)}},"pagination":{"limit":200,"hasMore":false,"nextCursor":null}}))
   },
   Endpoint::OwnedOrder{order_id}=>{
    self.reads.push(format!("order:{order_id}"));let id=Self::qid(&order_id);let q=&self.quotes[&id];
    let open=!self.server_closed.contains(&order_id)&&self.server_fills.get(&order_id).copied().unwrap_or(0)<10&&self.now<q["expiresAt"].as_u64().unwrap();
    (format!("/orders/{order_id}"),self.row(id,open))
   },
   Endpoint::OwnedFills{order_id}=>{
    self.reads.push(format!("fills:{order_id}"));
    if let Some(n)=self.fill_on_read.remove(&order_id){self.server_fills.insert(order_id.clone(),n);}
    let n=self.server_fills.get(&order_id).copied().unwrap_or(0);
    let rows=if n>0{vec![json!({"id":(2000+Self::qid(&order_id)).to_string(),"quantity":n,"side":"yes","price":0.45,"filledAt":iso(base()+500)})]}else{vec![]};
    let mut data=json!({"orderId":order_id,"exchangeId":"101","tournamentId":T,"totalQuantityFilled":n,"data":rows,"coverage":{"complete":true,"projectedThroughSequence":self.now},"pagination":{"limit":200,"hasMore":false,"nextCursor":null}});
    if self.omit_fill_limit{data["pagination"].as_object_mut().unwrap().remove("limit");}
    if self.bad_first_fill{self.bad_first_fill=false;data["coverage"]["projectedThroughSequence"]=json!(0);}
    (format!("/orders/{order_id}/fills?limit=200"),data)
   },
   _=>panic!("unexpected endpoint"),
  };
  Ok(Receipt{endpoint:path,status:200,request_started_at:iso(start),received_at:iso(self.now),data})
 }
 fn quote(&self,id:u64)->Result<Value>{Ok(self.quotes.get(&id).ok_or("missing quote")?.clone())}
 fn apply(&mut self,e:&Value)->Result<()>{
  if self.fail_apply{return Err("INJECTED_NATIVE_DURABILITY_FAILURE".into())}
  let fill=e["fillId"].as_str().ok_or("fill id")?.to_string();
  if self.seen.insert(fill){let q=self.quotes.get_mut(&e["orderId"].as_u64().unwrap()).unwrap();let n=q["filled"].as_u64().unwrap()+e["quantity"].as_u64().unwrap();q["filled"]=json!(n);q["retired"]=json!(n==q["quantity"].as_u64().unwrap());self.applied+=1;}
  Ok(())
 }
 fn close(&mut self,id:u64,e:&Value,order:&Receipt,fills:&Receipt)->Result<()>{
  assert!(chrono::DateTime::parse_from_rfc3339(&fills.request_started_at).unwrap()>=chrono::DateTime::parse_from_rfc3339(&order.received_at).unwrap());assert_eq!(e["filledQuantity"],self.quotes[&id]["filled"]);self.closed.insert(id);self.quotes.get_mut(&id).unwrap()["retired"]=json!(true);Ok(())
 }
}

#[test]fn twenty_open_orders_use_21_operations_and_all_twenty_full_tapes(){let mut b=Fake::new(20);let mut c=Reconciler::default();assert_eq!(c.retained_fill_watermarks(),0);let s=c.cycle(&scope(),&b.pending(),false,&mut b).unwrap();assert_eq!(s.aggregate_reads,1);assert_eq!(s.exact_order_reads,0);assert_eq!(s.complete_fill_reads,20);assert_eq!(s.validated_member_observations,20);assert_eq!(b.reads.len(),21);assert_eq!(b.reads.iter().filter(|r|r.starts_with("fills:")).count(),20);assert_eq!(c.retained_fill_watermarks(),20);assert!(b.closed.is_empty());}
#[test]fn sixty_second_prior_loop_gap_does_not_make_new_aggregate_stale(){let mut b=Fake::new(20);let mut c=Reconciler::default();c.cycle(&scope(),&b.pending(),false,&mut b).unwrap();b.now+=60_000;b.reads.clear();let s=c.cycle(&scope(),&b.pending(),false,&mut b).unwrap();assert_eq!(s.exact_order_reads,0);assert_eq!(s.complete_fill_reads,20);assert_eq!(b.reads.len(),21);}
#[test]fn slow_cohort_uses_exact_pairs_for_later_members(){let mut b=Fake::new(20);b.gap=1500;let mut c=Reconciler::default();let s=c.cycle(&scope(),&b.pending(),false,&mut b).unwrap();assert!(s.exact_order_reads>0);assert!(s.validated_member_observations>0);assert_eq!(s.complete_fill_reads,20);assert_eq!(s.validated_member_observations+s.exact_order_reads,20);assert!(b.closed.is_empty());}
#[test]fn queue_wait_stales_hint_then_exact_order_and_later_full_tape_are_required(){let mut b=Fake::new(2);b.first_fill_queue_wait=25_000;b.fill_on_read.insert("1001".into(),3);let mut c=Reconciler::default();let s=c.cycle(&scope(),&b.pending(),false,&mut b).unwrap();assert_eq!(s.hint_fallbacks,2);assert_eq!(s.exact_order_reads,2);assert_eq!(s.complete_fill_reads,3);assert_eq!(&b.reads[..4],&["aggregate","fills:1001","order:1001","fills:1001"]);assert_eq!(b.applied,1);assert_eq!(b.quotes[&1]["filled"],3);}
#[test]fn invalid_member_tape_is_not_applied_before_exact_fallback(){let mut b=Fake::new(2);b.bad_first_fill=true;b.fill_on_read.insert("1001".into(),3);let mut c=Reconciler::default();let s=c.cycle(&scope(),&b.pending(),false,&mut b).unwrap();assert_eq!(s.hint_fallbacks,1);assert_eq!(s.exact_order_reads,1);assert_eq!(s.complete_fill_reads,3);assert_eq!(b.applied,1);assert_eq!(b.quotes[&1]["filled"],3);}
#[test]fn complete_fill_tape_does_not_forge_terminal_order_proof(){let mut b=Fake::new(2);b.fill_on_read.insert("1001".into(),10);let mut c=Reconciler::default();let s=c.cycle(&scope(),&b.pending(),false,&mut b).unwrap();assert_eq!(s.exact_order_reads,0);assert_eq!(b.quotes[&1]["retired"],true);assert!(!b.closed.contains(&1));let s=c.cycle(&scope(),&b.pending(),false,&mut b).unwrap();assert!(s.exact_order_reads>=1);assert!(b.closed.contains(&1));assert_eq!(b.applied,1);}
#[test]fn canceled_missing_member_keeps_exact_terminal_reconciliation(){let mut b=Fake::new(2);b.server_closed.insert("1001".into());let mut c=Reconciler::default();let s=c.cycle(&scope(),&b.pending(),false,&mut b).unwrap();assert_eq!(s.exact_order_reads,1);assert_eq!(s.complete_fill_reads,2);assert!(b.closed.contains(&1));}
#[test]fn invalid_aggregate_falls_back_but_never_authorizes_new_submissions(){let mut b=Fake::new(2);b.aggregate_bad=true;let mut c=Reconciler::default();let s=c.cycle(&scope(),&b.pending(),false,&mut b).unwrap();assert_eq!(s.exact_order_reads,2);assert_eq!(s.complete_fill_reads,2);assert!(s.blocks_new_submissions);}
#[test]fn foreign_open_order_blocks_new_submissions_and_preserves_all_owned_reads(){let mut b=Fake::new(2);b.foreign=true;let mut c=Reconciler::default();let s=c.cycle(&scope(),&b.pending(),false,&mut b).unwrap();assert!(s.blocks_new_submissions);assert_eq!(s.exact_order_reads,2);assert_eq!(s.complete_fill_reads,2);}
#[test]fn failed_native_commit_does_not_advance_fill_watermark(){let mut b=Fake::new(2);b.fill_on_read.insert("1001".into(),3);b.fail_apply=true;let mut c=Reconciler::default();assert!(c.cycle(&scope(),&b.pending(),false,&mut b).is_err());assert_eq!(c.retained_fill_watermarks(),0);assert_eq!(b.quotes[&1]["filled"],0);}
#[test]fn cache_failure_cannot_discard_durable_fills_or_starve_other_orders(){let mut b=Fake::new(2);b.omit_fill_limit=true;b.fill_on_read.insert("1001".into(),3);b.fill_on_read.insert("1002".into(),2);let mut c=Reconciler::default();let s=c.cycle(&scope(),&b.pending(),false,&mut b).unwrap();assert_eq!(s.watermark_invalidations,2);assert_eq!(b.applied,2);assert_eq!(b.quotes[&1]["filled"],3);assert_eq!(b.quotes[&2]["filled"],2);assert_eq!(c.retained_fill_watermarks(),0);}
#[test]fn aggregate_durable_failure_propagates_without_following_network_reads(){let mut b=Fake::new(2);b.aggregate_error=Some("DURABILITY_FAILURE");let mut c=Reconciler::default();assert!(c.cycle(&scope(),&b.pending(),false,&mut b).is_err());assert_eq!(b.reads,vec!["aggregate"]);}
#[test]fn single_pending_order_retains_original_exact_pair_without_aggregate(){let mut b=Fake::new(1);let mut c=Reconciler::default();let s=c.cycle(&scope(),&b.pending(),false,&mut b).unwrap();assert_eq!(s.aggregate_reads,0);assert_eq!(s.exact_order_reads,1);assert_eq!(s.complete_fill_reads,1);assert_eq!(b.reads,vec!["order:1001","fills:1001"]);}
#[test]fn empty_pending_and_restart_have_no_fill_skip_authority(){let mut b=Fake::new(0);let mut c=Reconciler::default();assert_eq!(c.cycle(&scope(),&[],false,&mut b).unwrap().pending_orders,0);assert!(b.reads.is_empty());assert_eq!(Reconciler::default().retained_fill_watermarks(),0);}
