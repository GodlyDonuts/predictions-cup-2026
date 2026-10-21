//! FIFO ingress bounded by both message count and encoded payload weight.
//! Dropping or consuming a queued value releases its exact reservation.
use std::sync::{Arc,atomic::{AtomicUsize,Ordering},mpsc};
use std::time::Duration;

struct Queued<T>{value:Option<T>,weight:usize,used:Arc<AtomicUsize>}
impl<T> Drop for Queued<T>{fn drop(&mut self){self.used.fetch_sub(self.weight,Ordering::AcqRel);}}
pub struct Sender<T>{inner:mpsc::SyncSender<Queued<T>>,used:Arc<AtomicUsize>,maximum:usize,weight:fn(&T)->usize}
pub struct Receiver<T>{inner:mpsc::Receiver<Queued<T>>}
pub fn channel<T>(count:usize,maximum:usize,weight:fn(&T)->usize)->(Sender<T>,Receiver<T>){
    let(tx,rx)=mpsc::sync_channel(count);let used=Arc::new(AtomicUsize::new(0));
    (Sender{inner:tx,used,maximum,weight},Receiver{inner:rx})
}
impl<T> Sender<T>{
    pub fn try_send(&self,value:T)->Result<(),mpsc::TrySendError<T>>{
        let weight=(self.weight)(&value);
        if self.used.fetch_update(Ordering::AcqRel,Ordering::Acquire,|used|
            used.checked_add(weight).filter(|next|*next<=self.maximum)).is_err(){return Err(mpsc::TrySendError::Full(value))}
        let queued=Queued{value:Some(value),weight,used:self.used.clone()};
        self.inner.try_send(queued).map_err(|error|match error{
            mpsc::TrySendError::Full(mut queued)=>mpsc::TrySendError::Full(queued.value.take().unwrap()),
            mpsc::TrySendError::Disconnected(mut queued)=>mpsc::TrySendError::Disconnected(queued.value.take().unwrap()),
        })
    }
}
impl<T> Receiver<T>{
    pub fn try_recv(&self)->Result<T,mpsc::TryRecvError>{self.inner.try_recv().map(|mut queued|queued.value.take().unwrap())}
    pub fn recv_timeout(&self,timeout:Duration)->Result<T,mpsc::RecvTimeoutError>{self.inner.recv_timeout(timeout).map(|mut queued|queued.value.take().unwrap())}
    #[cfg(test)]pub fn try_iter(&self)->impl Iterator<Item=T>+'_ {std::iter::from_fn(||self.try_recv().ok())}
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn byte_limit_and_fifo_resume_after_consumption(){
        let(tx,rx)=channel(10,7,|x:&String|x.len());
        tx.try_send("abc".into()).unwrap();tx.try_send("defg".into()).unwrap();
        assert!(matches!(tx.try_send("h".into()),Err(mpsc::TrySendError::Full(_))));
        assert_eq!(rx.try_recv().unwrap(),"abc");assert_eq!(tx.used.load(Ordering::Acquire),4);
        tx.try_send("hij".into()).unwrap();assert_eq!(rx.try_recv().unwrap(),"defg");assert_eq!(rx.try_recv().unwrap(),"hij");
        assert_eq!(tx.used.load(Ordering::Acquire),0);
    }
    #[test]fn count_failure_disconnect_and_oversize_release_reservations(){
        let(tx,rx)=channel(1,100,|x:&String|x.len());tx.try_send("first".into()).unwrap();
        assert!(matches!(tx.try_send("second".into()),Err(mpsc::TrySendError::Full(_))));assert_eq!(tx.used.load(Ordering::Acquire),5);
        assert!(matches!(tx.try_send("x".repeat(101)),Err(mpsc::TrySendError::Full(_))));assert_eq!(tx.used.load(Ordering::Acquire),5);
        drop(rx);assert_eq!(tx.used.load(Ordering::Acquire),0);
        assert!(matches!(tx.try_send("after".into()),Err(mpsc::TrySendError::Disconnected(_))));assert_eq!(tx.used.load(Ordering::Acquire),0);
    }
}
