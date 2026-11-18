use agy_auth_rust::{crypto,model::{Credential,Registry,Account},paths::atomic_write};
use chrono::Utc;
use sha2::{Digest,Sha256};

#[test]
fn decrypt_existing_go_aes_gcm_format() {
    let dir=tempfile::tempdir().unwrap();
    let key=dir.path().join("encryption.key");
    atomic_write(&key,include_bytes!("fixtures/go-compatible.key")).unwrap();
    let encrypted=include_bytes!("fixtures/go-compatible.snapshot");
    let expected=include_bytes!("fixtures/go-compatible.plain");
    assert_eq!(&crypto::decrypt(&key,encrypted).unwrap(),expected);
}

#[test]
fn ids_match_original_go_scoped_account_algorithm(){
    let data=br#"{"token":{"refresh_token":"refresh-secret"},"auth_method":"consumer"}"#;
    let c=Credential::from_bytes(data.to_vec()).unwrap();
    let first=format!("{:x}",Sha256::digest(c.fingerprint.as_bytes()));
    assert_eq!(c.id(),format!("acct_{}",&first[..10]));
    let b=c.with_gcp("my-project","global").unwrap();
    let expected=format!("{:x}",Sha256::digest(format!("{}:gcp:my-project",b.fingerprint).as_bytes()));
    assert_eq!(b.id(),format!("acct_{}",&expected[..10]));
}

#[test]
fn selector_exact_alias_precedes_fuzzy_email() {
    let mut r=Registry::default();
    let a=Account{ id:"acct_a123456789".into(),email:"other-alias@example.org".into(),added_at:Utc::now(),..Default::default()};
    let b=Account{ id:"acct_b123456789".into(),alias:"other".into(),added_at:Utc::now(),..Default::default()};
    r.accounts=vec![a,b];r.normalize().unwrap();
    assert_eq!(r.accounts[r.index("other").unwrap()].alias,"other");
}

#[test]
fn rejects_tampered_old_ciphertext(){
    let dir=tempfile::tempdir().unwrap();
    let key=dir.path().join("encryption.key");
    atomic_write(&key,include_bytes!("fixtures/go-compatible.key")).unwrap();
    let mut e=include_bytes!("fixtures/go-compatible.snapshot").to_vec();
    let idx=e.len()-3;e[idx]=if e[idx]==b'a'{b'b'}else{b'a'};
    assert!(crypto::decrypt(&key,&e).is_err());
}
