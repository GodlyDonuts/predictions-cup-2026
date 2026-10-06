// The existing Rust publication validator is compiled into this process;
// there is no Node bridge or per-book child process.
include!("../../src/publication.rs");

pub struct Publisher { inner: Publication }
impl Publisher {
    pub fn new(universe: &Value) -> Result<Self,String> { Ok(Self { inner:Publication::new(universe)? }) }
    pub fn apply_record(&mut self,row:&Value)->Result<(),String>{self.inner.apply(row)}
    pub fn next_record(&self,kind:&str,payload:Value,at:&str,session_id:&str)->Result<Value,String>{
        let mut row=json!({"schemaVersion":1,"kind":kind,"sequence":self.inner.sequence+1,"previousHash":self.inner.hash,"sessionId":session_id,"tournamentId":self.inner.tournament,"scopeHash":self.inner.scope_hash,"at":at,"payload":payload});
        let digest=hash(&row)?;row["hash"]=json!(digest);Ok(row)
    }
    pub fn status(&self)->Value{json!({"schemaVersion":1,"tournamentId":self.inner.tournament,"scope":self.inner.scope,"scopeHash":self.inner.scope_hash,"sessionId":self.inner.session,"sessions":self.inner.sessions,"closed":self.inner.closed,"sequence":self.inner.sequence,"hash":self.inner.hash,"lastAt":self.inner.last_at,"books":self.inner.books,"sourceVersions":self.inner.versions,"invalidations":{"global":self.inner.global,"markets":self.inner.markets}})}
    pub fn from_verified_prefix(path:&Path,bytes:u64,sha:&str,universe:&Value)->Result<Self,String>{
        let result=read_prefix(path,bytes,sha,universe)?;let v=&result["state"];
        let strings=|key:&str|->Result<Vec<String>,String>{v[key].as_array().ok_or("SERVICE_IMPORT_PUBLICATION")?.iter().map(|x|x.as_str().map(str::to_owned).ok_or("SERVICE_IMPORT_PUBLICATION".into())).collect()};
        let map=|key:&str|->Result<Map<String,Value>,String>{v[key].as_object().cloned().ok_or("SERVICE_IMPORT_PUBLICATION".into())};
        Ok(Self{inner:Publication{scope:v["scope"].clone(),scope_hash:text(v,"scopeHash")?.into(),tournament:text(v,"tournamentId")?.into(),session:v["sessionId"].as_str().map(str::to_owned),sessions:strings("sessions")?,closed:v["closed"].as_bool().ok_or("SERVICE_IMPORT_PUBLICATION")?,sequence:v["sequence"].as_i64().ok_or("SERVICE_IMPORT_PUBLICATION")?,hash:text(v,"hash")?.into(),last_at:v["lastAt"].as_str().map(str::to_owned),books:map("books")?,versions:map("sourceVersions")?,global:v["invalidations"]["global"].clone(),markets:v["invalidations"]["markets"].as_object().cloned().ok_or("SERVICE_IMPORT_PUBLICATION")?}})
    }
}
