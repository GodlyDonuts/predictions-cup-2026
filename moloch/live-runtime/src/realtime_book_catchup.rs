//! Prospective selection AFTER every original public frame has been recorded and
//! successfully normalized. This is not a source validator or financial proof.
//! Only a complete realtime book can supersede an earlier book of the same exact
//! scope; every other event is a causal barrier. No clock or body is rewritten.
use serde_json::Value;

pub(super) type Stamped = (Value, u64);
const MAX_BOOKS: usize = 64;
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_ORIGINAL_EVENTS: usize = 256;

enum Input { Event(Stamped), Barrier }

// Account updates are on a separate socket path. This fence proves only that
// no generation change was observed during this drain, not cross-topic order.
pub(super) struct Drain {
    original: Vec<Input>,
    generation: Option<u64>,
    stable: bool,
    bytes: usize,
    events: usize,
    passthrough: bool,
}
impl Drain {
    pub(super) fn new(generation: Option<u64>) -> Self {
        Self { original:Vec::new(), generation, stable:true, bytes:0, events:0, passthrough:false }
    }
    pub(super) fn observe_generation(&mut self, generation: Option<u64>) {
        if generation != self.generation { self.stable=false; }
    }
    pub(super) fn frame(&mut self, events: Vec<Value>, at:u64, book_only:bool) -> Vec<Stamped> {
        if self.passthrough { return events.into_iter().map(|e|(e,at)).collect(); }
        if !book_only { self.original.push(Input::Barrier); }
        let mut out=Vec::new();
        for e in events {
            let bytes=e.to_string().len();
            if self.passthrough { out.push((e,at)); continue; }
            if self.events==MAX_ORIGINAL_EVENTS || bytes>MAX_BYTES.saturating_sub(self.bytes) {
                // Overflow is an optimization fallback, never source rejection.
                // All originals survive and this whole drain remains unselected.
                out.extend(self.take_original());self.passthrough=true;out.push((e,at));continue;
            }
            self.events+=1;self.bytes+=bytes;self.original.push(Input::Event((e,at)));
        }
        out
    }
    fn take_original(&mut self)->Vec<Stamped> {
        self.bytes=0;self.events=0;
        std::mem::take(&mut self.original).into_iter().filter_map(|i|match i{Input::Event(e)=>Some(e),Input::Barrier=>None}).collect()
    }
    pub(super) fn finish(&mut self, generation:impl Fn()->Option<u64>, success:bool)->Vec<Stamped> {
        self.observe_generation(generation());
        if !success || !self.stable || self.passthrough { return self.take_original(); }
        let mut books=Books::default();let mut out=Vec::new();
        for input in &self.original {
            match input { Input::Barrier=>out.extend(books.flush()), Input::Event((e,at))=>out.extend(books.push(e.clone(),*at)) }
        }
        out.extend(books.flush());
        // Selection itself may take time. Retain originals until this final
        // fence; a change during selection also restores every original event.
        self.observe_generation(generation());
        if !self.stable{return self.take_original();}
        self.original.clear();self.bytes=0;self.events=0;out
    }
}

#[derive(Default)]
pub(super) struct Books {
    pending: Vec<Stamped>,
    bytes: usize,
}

// Conservative raw-frame barrier, including facts outside the selected scopes.
// Missing/unknown shapes never authorize coalescing. RestSource still validates
// EVERY frame, including all intermediate revision/body conflicts.
pub(super) fn book_only_frame(event: &str, payload: &Value) -> bool {
    event == "markets_batch" && payload["markets"].as_array().is_some_and(|markets| {
        markets.iter().all(|m| {
            let b = &m["batch"];
            b["resyncRequired"] != true && ["trades", "bookDirty", "marketSettled"]
                .iter().all(|k| b[*k].as_array().is_some_and(Vec::is_empty))
        })
    })
}

fn key(e: &Value) -> Option<(&str, &str, &str)> {
    let b = &e["book"];
    if e["type"] != "book" || b["invalid"] == true || b["transport"] != "official_sig_realtime"
        || !["bids", "asks"].iter().all(|k| b[*k].as_array().is_some_and(|v| !v.is_empty()))
        || !["sourceAt", "requestStartedAt", "receivedAt", "sequence"].iter()
            .all(|k| b[*k].as_str().is_some_and(|s| !s.is_empty()))
    { return None; }
    Some((b["tournamentId"].as_str()?, b["marketId"].as_str()?, b["exchangeId"].as_str()?))
}

impl Books {
    pub(super) fn flush(&mut self) -> Vec<Stamped> {
        self.bytes = 0;
        std::mem::take(&mut self.pending)
    }

    pub(super) fn push(&mut self, event: Value, at: u64) -> Vec<Stamped> {
        let Some(scope) = key(&event) else {
            let mut out = self.flush(); out.push((event, at)); return out;
        };
        // Serialization is only a memory bound, never an authority/hash witness.
        let bytes = event.to_string().len();
        let mut out = Vec::new();
        if bytes > MAX_BYTES {
            out = self.flush(); out.push((event, at)); return out;
        }
        let old = self.pending.iter().position(|(e, _)| key(e) == Some(scope));
        let old_bytes = old.map_or(0, |i| self.pending[i].0.to_string().len());
        if (old.is_none() && self.pending.len() == MAX_BOOKS)
            || self.bytes - old_bytes + bytes > MAX_BYTES {
            out = self.flush();
        } else if let Some(i) = old {
            self.pending.remove(i); self.bytes -= old_bytes;
        }
        // Latest-occurrence order, not exchange sort order; actual source clocks
        // and the original normalization-time processing stamp travel unchanged.
        self.bytes += bytes; self.pending.push((event, at)); out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn book(ex: &str, sequence: u64) -> Value {
        json!({"type":"book","receivedAt":"2026-10-17T15:00:01.000Z","book":{
            "tournamentId":"cup","marketId":format!("m{ex}"),"exchangeId":ex,
            "sequence":sequence.to_string(),"sourceAt":"2026-10-17T15:00:00.123456789Z",
            "requestStartedAt":"2026-10-17T15:00:01.000Z","receivedAt":"2026-10-17T15:00:01.000Z",
            "transport":"official_sig_realtime","bids":[{"price":"0.4","quantity":13}],
            "asks":[{"price":"0.6","quantity":17}]}})
    }
    #[test] fn latest_actual_book_and_original_stamp_survive_in_last_occurrence_order() {
        let mut b=Books::default();let first=book("1",1);let other=book("2",7);let last=book("1",2);
        for (e,at) in [(first,100),(other.clone(),101),(last.clone(),102)] {assert!(b.push(e,at).is_empty());}
        assert_eq!(b.flush(),vec![(other,101),(last,102)]);
    }
    #[test] fn every_nonbook_barrier_preserves_the_preceding_book() {
        for kind in ["trade","gap","coverage","actual_fill","account_dirty","settlement","unknown"] {
            let mut b=Books::default();let first=book("1",1);let barrier=json!({"type":kind,"id":"actual"});
            b.push(first.clone(),100);assert_eq!(b.push(barrier.clone(),101),vec![(first,100),(barrier,101)]);
            let last=book("1",2);b.push(last.clone(),102);assert_eq!(b.flush(),vec![(last,102)]);
        }
    }
    #[test] fn invalid_rest_or_incomplete_books_cannot_supersede_valid_depth() {
        for change in [json!({"invalid":true}),json!({"transport":"official_sig_api_v1"}),json!({"bids":[]}),json!({"sourceAt":null})] {
            let mut b=Books::default();let first=book("1",1);let mut bad=book("1",2);
            for (k,v) in change.as_object().unwrap(){bad["book"][k]=v.clone();}
            b.push(first.clone(),100);assert_eq!(b.push(bad.clone(),101),vec![(first,100),(bad,101)]);
        }
    }
    #[test] fn differing_market_or_tournament_never_aliases_an_exchange() {
        let mut b=Books::default();let first=book("1",1);let mut changed=book("1",2);changed["book"]["marketId"]=json!("other");
        b.push(first.clone(),100);b.push(changed.clone(),101);assert_eq!(b.flush(),vec![(first,100),(changed,101)]);
    }
    #[test] fn raw_fact_or_dirty_barrier_is_kept_even_when_normalizer_has_no_scoped_output() {
        let clean=json!({"markets":[{"batch":{"books":[],"trades":[],"bookDirty":[],"marketSettled":[]}}]});
        assert!(book_only_frame("markets_batch",&clean));
        for field in ["trades","bookDirty","marketSettled"]{let mut raw=clean.clone();raw["markets"][0]["batch"][field]=json!([{"outsideScope":true}]);assert!(!book_only_frame("markets_batch",&raw));}
        for event in ["book_dirty","market_settled","joined","failed"]{assert!(!book_only_frame(event,&clean));}
        let mut raw=clean.clone();raw["markets"][0]["batch"]["resyncRequired"]=json!(true);assert!(!book_only_frame("markets_batch",&raw));
        assert!(!book_only_frame("markets_batch",&Value::Null));
    }
    #[test] fn count_and_byte_bounds_flush_without_dropping_an_actual_event() {
        let mut b=Books::default();for i in 0..MAX_BOOKS{assert!(b.push(book(&i.to_string(),1),i as u64).is_empty());}
        assert_eq!(b.push(book("overflow",1),999).len(),MAX_BOOKS);assert_eq!(b.flush().len(),1);
        let mut huge=book("large",1);huge["originalExtra"]=json!("x".repeat(MAX_BYTES+1));
        let prior=book("1",1);b.push(prior.clone(),1);assert_eq!(b.push(huge.clone(),2),vec![(prior,1),(huge,2)]);assert!(b.flush().is_empty());
    }
    #[test] fn barrier_flush_on_failure_or_end_keeps_the_last_real_book() {
        let mut b=Books::default();let e=book("1",1);b.push(e.clone(),42);assert_eq!(b.flush(),vec![(e,42)]);assert!(b.flush().is_empty());
    }
    #[test] fn stable_enabled_account_generation_permits_selection_but_any_observed_change_keeps_all() {
        let first=book("1",1);let last=book("1",2);
        for change in [0,1,2,3] {
            let mut d=Drain::new(Some(7));
            if change==1{d.observe_generation(Some(8));}
            assert!(d.frame(vec![first.clone()],100,true).is_empty());
            if change==2{d.observe_generation(Some(8));d.observe_generation(Some(7));}
            assert!(d.frame(vec![last.clone()],101,true).is_empty());
            let out=d.finish(||if change==3{Some(8)}else{Some(7)},true);
            assert_eq!(out,if change==0{vec![(last.clone(),101)]}else{vec![(first.clone(),100),(last.clone(),101)]});
        }
    }
    #[test] fn empty_out_of_scope_raw_barrier_and_failure_cannot_hide_originals() {
        let first=book("1",1);let last=book("1",2);let mut d=Drain::new(None);
        d.frame(vec![first.clone()],100,true);d.frame(vec![],101,false);d.frame(vec![last.clone()],102,true);
        assert_eq!(d.finish(||None,true),vec![(first.clone(),100),(last.clone(),102)]);
        let mut d=Drain::new(Some(1));d.frame(vec![first.clone(),last.clone()],100,true);
        assert_eq!(d.finish(||Some(1),false),vec![(first,100),(last,100)]);
    }
    #[test] fn generation_change_during_selection_restores_every_original() {
        let mut d=Drain::new(Some(7));let rows=vec![book("1",1),book("1",2)];d.frame(rows.clone(),100,true);
        let calls=std::cell::Cell::new(0);
        let out=d.finish(||{let n=calls.get();calls.set(n+1);Some(if n==0{7}else{8})},true);
        assert_eq!(calls.get(),2);assert_eq!(out,rows.into_iter().map(|e|(e,100)).collect::<Vec<_>>());
    }
    #[test] fn original_buffer_bound_falls_back_for_entire_drain_without_partial_selection() {
        let rows:Vec<_>=(0..MAX_ORIGINAL_EVENTS+2).map(|i|book("1",i as u64)).collect();
        let mut d=Drain::new(Some(1));let mut out=d.frame(rows.clone(),42,true);
        out.extend(d.frame(vec![book("1",999)],43,true));out.extend(d.finish(||Some(1),true));
        let mut expected:Vec<_>=rows.into_iter().map(|e|(e,42)).collect();expected.push((book("1",999),43));assert_eq!(out,expected);
        let mut d=Drain::new(Some(1));let first=book("1",1);d.frame(vec![first.clone()],10,true);
        let mut huge=book("1",2);huge["originalExtra"]=json!("x".repeat(MAX_BYTES+1));
        let mut out=d.frame(vec![huge.clone()],11,true);out.extend(d.finish(||Some(1),true));assert_eq!(out,vec![(first,10),(huge,11)]);
    }
    #[test] fn genuine_normalizer_checks_intermediate_originals_before_any_selection() {
        use moloch_replay::source::RestSource;
        let mut s=RestSource::new(&json!([{"marketId":"1","exchangeId":"2"}]),"cup","2026-10-14T00:00:00Z").unwrap();
        let receipt=json!({"kind":"book","source":"official_sig_api_v1","status":200,"marketId":"1","exchangeId":"2","tournamentId":"cup","requestStartedAt":"2026-10-14T00:00:00Z","receivedAt":"2026-10-14T00:00:01Z","endpoint":"/markets/1/orderbook?tournamentId=cup&depth=200","data":{"contexts":[{"type":"tournament","tournament":{"id":"cup"},"orderbook":{"exchanges":[{"exchangeId":"2","asOf":{"sequence":100,"at":"2026-10-14T00:00:00Z"},"bids":[{"price":0.45,"quantity":100}],"asks":[{"price":0.455,"quantity":100}]}]}}]}});
        s.accept(&receipt).unwrap();
        s.accept(&json!({"kind":"trades","source":"official_sig_api_v1","status":200,"marketId":"1","exchangeId":"2","tournamentId":"cup","requestStartedAt":"2026-10-14T00:01:00Z","receivedAt":"2026-10-14T00:01:01Z","endpoint":"/exchanges/2/trades?tournamentId=cup&from=2026-10-14T00:00:00Z&to=2026-10-14T00:01:00Z","data":{"marketId":"1","exchangeId":"2","from":"2026-10-14T00:00:00Z","to":"2026-10-14T00:01:00Z","data":[],"pagination":{"hasMore":false,"nextCursor":null},"coverage":{"complete":true,"projectedThroughSequence":100}}})).unwrap();
        let frame=|rev:u64,seq:u64,bid:f64|json!({"markets":[{"marketId":"1","batch":{"delivery":{"model":"best-effort-authoritative-resync","revision":rev,"previousRevision":rev-1,"sourceSequenceFrom":seq,"sourceSequenceThrough":seq,"correlationId":"fixture"},"books":[{"exchangeId":"2","asOf":{"sequence":seq,"at":"2026-10-14T00:01:01Z"},"nextExpiryAt":null,"bids":[{"price":bid,"quantity":100}],"asks":[{"price":0.455,"quantity":100}]}],"trades":[],"bookDirty":[],"marketSettled":[]}}]});
        let mut selected=Drain::new(Some(7));let mut original=Vec::new();
        for revision in 1..=3 {
            let raw=frame(revision,100+revision,0.45);let events=s.broadcast("markets_batch",&raw,"2026-10-14T00:01:02Z").unwrap();
            assert_eq!(events.len(),1);assert_eq!(events[0]["type"],"book");assert_ne!(events[0]["book"]["invalid"],true);
            original.extend(events.clone().into_iter().map(|e|(e,revision)));
            assert!(selected.frame(events,revision,book_only_frame("markets_batch",&raw)).is_empty());
        }
        // Genuine three accepted complete books -> exactly the third, unchanged.
        assert_eq!(selected.finish(||Some(7),true),vec![original[2].clone()]);
        let mut failed=Drain::new(Some(7));for(e,at)in &original{failed.frame(vec![e.clone()],*at,true);}
        let conflict=s.broadcast("markets_batch",&frame(4,103,0.445),"2026-10-14T00:01:02Z").unwrap_err();
        assert_eq!(conflict,"SOURCE_BOOK_CONFLICT");assert_eq!(failed.finish(||Some(7),false),original);
        // A missing delivery revision produces a real gap; it remains a barrier,
        // rather than guessing the latest book repairs continuity.
        let mut raw=frame(6,104,0.45);raw["markets"][0]["batch"]["delivery"]["previousRevision"]=json!(5);
        let gap=s.broadcast("markets_batch",&raw,"2026-10-14T00:01:02Z").unwrap();assert!(gap.iter().any(|e|e["type"]=="gap"));
        let mut d=Drain::new(Some(7));d.frame(vec![original[2].0.clone()],3,true);d.frame(gap.clone(),4,true);
        let out=d.finish(||Some(7),true);assert_eq!(out[0],original[2]);assert_eq!(out[1].0,gap[0]);
    }
}
