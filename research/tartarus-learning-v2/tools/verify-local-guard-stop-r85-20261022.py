"""Verify the stopped R83 worker's original no-HTTP evidence without account requests."""
from pathlib import Path
import datetime,hashlib,json,os,shutil,subprocess

W=Path('/tmp/moloch-local-guard-cohort-r85-20261022')
P=Path('/tmp/moloch-source-projection-r84-20261022')
R=Path('/home/archon/apps/moloch/shared/live-production-v1')
CONTROL=R.parent/'moloch-live-tartarus-r83-local-retirement-sweep-20261022.json'
RUST='/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'
SOURCE=r'''
use std::{fs,path::PathBuf};
use serde_json::{Value,json};
fn main()->Result<(),Box<dyn std::error::Error>>{
 let a:Vec<String>=std::env::args().collect();let c:Value=serde_json::from_slice(&fs::read(&a[1])?)?;
 let config=baal_transport_service::Config{queue_dir:PathBuf::from(c["root"].as_str().ok_or("root")?).join("account-queue"),account_scope:c["accountScope"].as_str().ok_or("accountScope")?.into(),scope:serde_json::from_value(c["scope"].clone())?,maximum_pages:1,writes_enabled:false};
 let mut log=moloch_live_runtime::dispatch::DispatchLog::open(std::path::Path::new(&a[2]))?;
 let aborted:Vec<_>=log.aborted_records().iter().map(|(id,e)|(*id,e["proof"].clone())).collect();
 for(id,e)in aborted{let(s,p,at,error)=log.pre_admission_originals(id)?;let proof=baal_transport_service::verify_retained_ordinary_pre_admission(&config,&s,&p,at,&error,&e)?;log.verify_aborted_before_admission(id,&proof)?;}
 let local:Vec<_>=log.local_final_guard_records().iter().map(|(id,e)|(*id,e["proof"].clone())).collect();
 for(id,e)in local{let(s,p,at,error)=log.local_final_guard_originals(id)?;let proof=baal_transport_service::verify_retained_ordinary_local_guard_no_http(&config,&s,&p,at,&error,&e)?;log.verify_local_final_guard_no_http(id,&proof)?;}
 let unclassified:Vec<_>=log.attempted.iter().filter(|id|!log.acknowledgments.contains_key(id)&&!log.proved_no_network(**id)&&!log.exposure_records().contains_key(id)).copied().collect();assert!(unclassified.is_empty());
 let retained_exposure:Vec<_>=log.exposure_records().keys().copied().collect();
 let id=20937;assert!(log.proved_no_network(id)&&log.local_final_guard_records().contains_key(&id)&&!log.aborted_records().contains_key(&id)&&!log.acknowledgments.contains_key(&id)&&!log.closed.contains(&id));
 println!("{}",json!({"qid":id,"originalKind":"local_final_guard_no_http","opaqueSdkProofVerified":true,"unclassifiedAttemptIds":unclassified,"historicalExposureQidsRequiringNativeStartupVerification":retained_exposure,"actualAcknowledgment":false,"retryAuthorized":false,"accountRequests":0}));Ok(())
}
'''

def sha(p):
    with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()

def main():
    assert not Path('/proc/1063142').exists() and not Path('/proc/1066414').exists()
    stop=(R/'EXECUTION_STOP').read_bytes();s=json.loads(stop)
    assert s['reason']=='FUNDAMENTAL_ABANDON_POSITIVE_NOT_SENT_REQUIRED' and s['at']=='2026-10-22T04:59:15.075Z'
    dispatch=sha(R/'dispatch.ndjson');copy=W/'original-dispatch-for-verification.ndjson';shutil.copy2(R/'dispatch.ndjson',copy);assert sha(copy)==dispatch
    source=W/'verify-original-stop.rs';source.write_text(SOURCE);binary=W/'verify-original-stop'
    args=json.loads((P/'worker-release-command.json').read_bytes());selected=[]
    for i,a in enumerate(args[:-1]):
        if a in ['-L','--extern','-C']:selected.extend([a,args[i+1]])
    with (W/'verify-original-stop-build.log').open('w') as log:subprocess.run([RUST,'--edition=2024',str(source),*selected,'-o',str(binary)],check=True,stdout=log,stderr=subprocess.STDOUT)
    proof=json.loads(subprocess.check_output([str(binary),str(CONTROL),str(copy)],text=True))
    b=R/'bank-all-markets-v1';report=json.loads((b/'report.json').read_bytes());q=next(q for q in report['orders'] if q['id']==20937)
    assert q['retired'] is True and q['filled']==0 and q['exchangeId']=='1075' and not any(f['orderId']==20937 for f in report['fills'])
    qr=R/'account-queue';manifest=json.loads((qr/'account-queue-manifest.json').read_bytes());journal=qr/manifest['journal']
    with journal.open('rb') as f:f.seek(max(0,journal.stat().st_size-16*1024*1024));tail=f.read()
    queue=json.loads(tail.splitlines()[-1])['state'];assert queue['active'] is None and not queue['unknown'] and queue['manual_review'] is False
    assert (R/'EXECUTION_STOP').read_bytes()==stop and sha(copy)==dispatch and sha(R/'dispatch.ndjson')==dispatch
    out=dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),previousPid=1063142,previousStartTicks=61099963,previousUpdaterPid=1066414,
        stop=s,stopSha256=hashlib.sha256(stop).hexdigest(),originalDispatchSha256=dispatch,originalReportSha256=sha(b/'report.json'),originalHeadSha256=sha(b/'head.json'),
        originalNoHttp=proof,retiredOriginalQuote=q,financial={k:report['report'][k] for k in ['cash','ownedCost','realized','newTradingRealized','inheritedRealizedGross']},
        queueActive=False,queueUnknownCount=0,queueManualReview=False,originalFillCount=len(report['fills']),
        helperSha256=sha(binary),helperSourceSha256=sha(source),stopCleared=False,financialStateChanged=False)
    (W/'stopped-incident.public.json').write_text(json.dumps(out,indent=2)+'\n');print(json.dumps(out),flush=True)

if __name__=='__main__':os.umask(0o077);main()
