//! Retain parsed historical response evidence before strict validation.
//! This is a typed GET using the original transport and account admission.
use super::*;
use std::{fs::OpenOptions, io::Write, path::Path};

impl Transport {
    pub fn request_history_with_evidence(&mut self, exchange_id:String, to:String, output:&Path)->Result<Receipt>{
        need(output.is_absolute()&&!output.exists(),"NEW_ABSOLUTE_HISTORY_EVIDENCE_REQUIRED")?;
        let endpoint=Endpoint::History{exchange_id,to};
        let path=self.path(&endpoint,None)?;
        let observed_endpoint=path.clone();
        self.perform(RequestSpec{method:"GET".into(),endpoint:path,body:None,body_json:None},None,|s,v|{
            retain(output,&observed_endpoint,s.clock.now_ms(),v)?;
            s.validate(&endpoint,v)
        })
    }
}

fn retain(path:&Path,endpoint:&str,received_ms:u64,data:&Value)->Result<()> {
    let bytes=serde_json::to_vec(&json!({"classification":"parsed_history_response_not_validated_not_raw_wire_bytes",
        "endpoint":endpoint,"receivedAt":utc(received_ms)?,"data":data,"trainingAdmitted":false})).map_err(|_|fail("HISTORY_EVIDENCE_SERIALIZE"))?;
    let mut options=OpenOptions::new();options.write(true).create_new(true);
    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
    let mut file=options.open(path).map_err(|_|fail("HISTORY_EVIDENCE_CREATE"))?;
    file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(|_|fail("HISTORY_EVIDENCE_PERSIST"))
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn evidence_preserves_arbitrary_decimal_and_refuses_overwrite(){
        let root=std::env::temp_dir().join(format!("sig-history-evidence-{}",std::process::id()));
        std::fs::create_dir(&root).unwrap();let path=root.join("receipt.json");
        let data:Value=serde_json::from_str("{\"vwap\":0.1234567890123456789}").unwrap();
        retain(&path,"/exchanges/840/price-history",1,&data).unwrap();
        let saved=std::fs::read(&path).unwrap();let v:Value=serde_json::from_slice(&saved).unwrap();
        assert_eq!(v["data"]["vwap"].to_string(),"0.1234567890123456789");
        assert_eq!(v["trainingAdmitted"],false);
        assert!(retain(&path,"same",2,&data).is_err());assert_eq!(saved,std::fs::read(&path).unwrap());
        assert_eq!(root.canonicalize().unwrap().parent().unwrap(),std::env::temp_dir().canonicalize().unwrap());
        std::fs::remove_file(path).unwrap();std::fs::remove_dir(root).unwrap();
    }
}
