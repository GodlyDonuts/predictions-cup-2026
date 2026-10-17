//! Original-source byte limits; synthetic documents grant no live approval.
use moloch_live_runtime::fundamental_policy::{
    ReviewFile, ReviewInput, Reviewed, Settings, SourceBytes, SourceFile,
    official_contract_root_sha256, review_sha256,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const MIB: usize = 1024 * 1024;
const LIMIT: usize = 16 * MIB;
const NOW: u64 = 1_792_108_800_000;

fn sha(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }

// A complete, single-page PDF with an original uncompressed content stream,
// proper object offsets/xref and EOF. The byte/hash test never substitutes a
// clipped page or normalized extraction for this original document.
fn original_pdf(minimum: usize) -> Vec<u8> {
    let mut body = b"q\n% synthetic General ballot fixture; not an official publisher\n".to_vec();
    while body.len() < minimum { body.extend_from_slice(b"% bounded original document content\n"); }
    body.extend_from_slice(b"Q\n");
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (id, object) in [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>".to_vec(),
        [format!("<< /Length {} >>\nstream\n", body.len()).into_bytes(), body, b"endstream".to_vec()].concat(),
    ].into_iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", id + 1).as_bytes());
        pdf.extend_from_slice(&object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for offset in &offsets[1..] { pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes()); }
    pdf.extend_from_slice(format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    pdf
}

struct Originals { review: ReviewInput, bodies: Vec<Vec<u8>>, hashes: Vec<String>, trusted: BTreeSet<String> }
impl Originals {
    fn new(ballot_minimum: usize, forecast_length: usize) -> Self {
        let node = json!({"market_id":"201","root":{
            "node_type":"contract","node_id":"301","contract_id":"301","contract_type":"Election Outcome",
            "contract_details":{"contractType":"Election Outcome","resolutionType":"Party Winner","winnerName":"Democratic Party",
                "electionDate":"2026-11-03","officeLevel":"Federal","usState":"Synthetic","raceId":1,"stageId":2,
                "raceName":"Synthetic General fixture","raceStage":"General"},
            "settled_with":null,"settlement_date":null,"title":"Synthetic Democratic Party Winner fixture"}});
        let bodies = vec![vec![b'f'; forecast_length], original_pdf(ballot_minimum), serde_json::to_vec(&node).unwrap()];
        let hashes = bodies.iter().map(|b| sha(b)).collect::<Vec<_>>();
        let sources = ["forecast", "official_ballot", "official_contract_node"].iter().enumerate().map(|(i, kind)| {
            json!({"kind":kind,"url":format!("https://fixture.invalid/{kind}"),"sha256":hashes[i],
                "publishedAtMs":NOW-100,"observedAtMs":NOW-50,"maximumAgeMs":120000})
        }).collect::<Vec<_>>();
        let review: ReviewInput = serde_json::from_value(json!({
            "version":1,"reviewerId":"source-bound-test","assumptionLabel":"Synthetic model and mapping only; no calibration or live approval",
            "runId":"source-bound-fixture","bankManifestSha256":"a".repeat(64),"tournamentId":"cup","marketId":"201","exchangeId":"101",
            "contractId":"301","side":"YES","officialContractRootSha256":official_contract_root_sha256(&node).unwrap(),
            "ordinaryProbability":"0.95","exceptionalProbabilityAllowance":"0.03","minimumExceptionalPayout":"0","marginPerShare":"0.01",
            "maximumPrincipal":"2000","maximumQuantity":10000,"reviewedAtMs":NOW,"expiresAtMs":NOW+60000,"holdUntilMs":NOW+3600000,
            "payoutMappingIsOperatorAssumption":true,"revokeExisting":false,"sources":sources
        })).unwrap();
        let trusted = [review_sha256(&review).unwrap()].into();
        Self { review, bodies, hashes, trusted }
    }
    fn verify(&self) -> moloch_live_runtime::feedback::Result<Reviewed> {
        let originals = self.bodies.iter().zip(&self.hashes).map(|(bytes, sha256)| SourceBytes { bytes, sha256 }).collect::<Vec<_>>();
        Reviewed::verify(self.review.clone(), &originals, &self.trusted, NOW)
    }
}

#[test]
fn source_bytes_accept_complete_original_pdf_above_old_eight_mib_limit() {
    let f = Originals::new(8 * MIB + 4096, 64);
    assert!(f.bodies[1].len() > 8 * MIB && f.bodies.iter().map(Vec::len).sum::<usize>() < LIMIT);
    assert!(f.bodies[1].starts_with(b"%PDF-1.4\n") && f.bodies[1].ends_with(b"%%EOF\n"));
    assert_eq!(f.verify().unwrap().sha256(), review_sha256(&f.review).unwrap());
}

#[test]
fn source_bytes_reject_single_original_larger_than_sixteen_mib() {
    let f = Originals::new(LIMIT + 1, 64);
    assert!(f.bodies[1].len() > LIMIT);
    assert!(f.verify().err().expect("oversized source rejected").to_string().contains("FUNDAMENTAL_ORIGINAL_SOURCE_HASH"));
}

#[test]
fn large_source_bytes_still_require_exact_original_hash() {
    let mut f = Originals::new(8 * MIB + 4096, 64);
    f.bodies[1][100] ^= 1;
    assert!(f.verify().err().expect("altered original rejected").to_string().contains("FUNDAMENTAL_ORIGINAL_SOURCE_HASH"));
}

#[cfg(unix)]
mod actual_settings {
    use super::*;
    use std::{fs, path::PathBuf, sync::atomic::{AtomicU64, Ordering}};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Files { root: PathBuf, settings: Settings }
    impl Files {
        fn new(f: &Originals) -> Self {
            let root = std::env::temp_dir().join(format!("moloch-source-bounds-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&root).unwrap();
            let source_files = f.bodies.iter().enumerate().map(|(i, bytes)| {
                let path = root.join(format!("original-{i}.bin")); fs::write(&path, bytes).unwrap();
                SourceFile { path, sha256: f.hashes[i].clone() }
            }).collect();
            let path = root.join("review.json"); let bytes = serde_json::to_vec(&f.review).unwrap(); fs::write(&path, &bytes).unwrap();
            let authority_path = root.join("authority.json");
            let authority = serde_json::to_vec(&json!({"version":1,"trustedReviewSha256":f.trusted})).unwrap();
            fs::write(&authority_path, &authority).unwrap();
            Self { root, settings: Settings { version: 1, authority_path, authority_sha256: sha(&authority),
                reviews: vec![ReviewFile { path, file_sha256: sha(&bytes), source_files }], allow_initialization: false } }
        }
    }
    impl Drop for Files { fn drop(&mut self) { fs::remove_dir_all(&self.root).unwrap(); } }

    #[test]
    fn settings_accept_large_original_and_retain_file_hash_pins() {
        let f = Originals::new(8 * MIB + 4096, 64); let files = Files::new(&f);
        let loaded = files.settings.load().unwrap(); loaded.validate_files().unwrap();
        let path = &files.settings.reviews[0].source_files[1].path;
        let mut original = fs::read(path).unwrap(); original[100] ^= 1; fs::write(path, original).unwrap();
        assert!(loaded.validate_files().is_err(), "held descriptor still detects original mutation");
        assert!(files.settings.load().is_err(), "new load still requires pinned raw hash");
    }

    #[test]
    fn settings_reject_single_over_sixteen_mib_before_admission() {
        let files = Files::new(&Originals::new(LIMIT + 1, 64));
        assert!(files.settings.load().err().expect("single source bound").to_string().contains("FUNDAMENTAL_FILE_BOUND"));
    }

    #[test]
    fn settings_keep_aggregate_sixteen_mib_when_each_original_fits() {
        let f = Originals::new(8 * MIB + 4096, 8 * MIB);
        assert!(f.bodies.iter().all(|b| b.len() <= LIMIT));
        assert!(f.bodies.iter().map(Vec::len).sum::<usize>() > LIMIT);
        let files = Files::new(&f);
        assert!(files.settings.load().err().expect("aggregate bound unchanged").to_string().contains("FUNDAMENTAL_SOURCE_BOUND"));
    }

    #[test]
    fn deleted_original_still_invalidates_loaded_and_fresh_settings() {
        let files = Files::new(&Originals::new(8 * MIB + 4096, 64));
        let loaded = files.settings.load().unwrap(); loaded.validate_files().unwrap();
        fs::remove_file(&files.settings.reviews[0].source_files[1].path).unwrap();
        assert!(loaded.validate_files().is_err()); assert!(files.settings.load().is_err());
    }
}
