//! Offline wallet oracle. No Session, SDK, credentials, worker or account I/O.
use moloch_replay::{Wallet, Reservation, Side};
use serde_json::{json, Value};
fn row(w:&Wallet,label:&str)->Value{json!({"label":label,"cash":w.cash.to_string(),"reserved":w.reserved().unwrap().to_string(),"owned_cost":w.owned_cost().to_string(),"realized":w.realized.to_string(),"fees":w.fees.to_string(),"yes":w.quantity("x",Side::Yes),"no":w.quantity("x",Side::No)})}
fn reserve(w:&mut Wallet,id:&str,side:Side,n:u64,p:i128){w.reserve(id,Reservation{exchange:"x".into(),side,quantity:n,limit:p}).unwrap();}
fn main(){
 let mut w=Wallet::new(1_000_000_000,1_000_000_000,0,1000).unwrap();let mut out=vec![row(&w,"initial")];
 reserve(&mut w,"yes",Side::Yes,10,400000);out.push(row(&w,"reserve_yes"));
 w.buy_fill_exact("yes",4,390000,4000).unwrap();out.push(row(&w,"partial_yes"));
 w.buy_fill_exact("yes",6,400000,6000).unwrap();out.push(row(&w,"complete_yes"));
 reserve(&mut w,"no",Side::No,6,550000);out.push(row(&w,"reserve_opposite"));
 w.buy_fill_exact("no",3,540000,3000).unwrap();out.push(row(&w,"partial_opposite_net"));
 w.release("no").unwrap();out.push(row(&w,"cancel_remainder"));
 w.sell_fill_exact("x",Side::Yes,2,600000,2000).unwrap();out.push(row(&w,"fifo_sale"));
 reserve(&mut w,"flat",Side::No,5,500000);out.push(row(&w,"reserve_flatten"));
 w.buy_fill_exact("flat",5,500000,5000).unwrap();out.push(row(&w,"complete_flatten"));
 println!("{}",json!({"classification":"offline_mechanics_differential_not_strategy_returns","rows":out}));
}
