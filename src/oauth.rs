use anyhow::{anyhow,bail,Context,Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD,Engine};
use chrono::{Duration as ChronoDuration,Utc};
use rand::{rngs::OsRng,RngCore};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{env,fs,io::{self,Write},path::Path,process::Command,time::Duration};
use crate::model::Credential;

const REDIRECT:&str="https://antigravity.google/oauth-callback";
const SCOPES:&str="https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/cclog https://www.googleapis.com/auth/experimentsandconfigs https://www.googleapis.com/auth/aicode openid";
const GCP_SCOPES:&str="https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/cclog https://www.googleapis.com/auth/experimentsandconfigs openid";
const CLIENT_ID_HASH:&str="bf00c418024ba6bf606ccdc37120976e41bc429dd1d46ecf16a729aa532626ea";
const CLIENT_SECRET_HASH:&str="1d2f041093fd95aa8995a038c711d50a7960da09a505381c09a745d6ad0ecc60";
const CLIENT_SECRET_HASH_V2:&str="23d89fc5fbce73ed5b9e0cf0ca786bd0eb1bcdff8f65d03dc022d299ad2234b6";
const GCP_ID_HASH:&str="69dee430c4e956f6b672b99a846566d6a86814999fc3b3b050697c0d65ea80c3";
const GCP_SECRET_HASH:&str="a2ddedc84850a9c48b7e01d61700848083bf0e10fb8d6c99c496803e8b48ced0";

fn random_url(bytes:usize)->String{let mut b=vec![0;bytes];OsRng.fill_bytes(&mut b);URL_SAFE_NO_PAD.encode(&b)}
fn sha(input:&[u8])->String{format!("{:x}",Sha256::digest(input))}
fn valid_id(b:u8)->bool{b.is_ascii_alphanumeric()||b==b'-'||b==b'_'}
fn valid_secret(b:u8)->bool{b.is_ascii_alphanumeric()||b==b'-'||b==b'_'}

// The original Go project authenticates using OAuth credentials embedded in the installed agy binary.
// The discovery mechanism below verifies hashes and does not embed those credentials in the executable.
fn discover_client(binary:&Path,gcp:bool)->Result<(String,String)>{
    let meta=fs::metadata(binary).with_context(||format!("read agy executable {}",binary.display()))?;
    if meta.len()>512*1024*1024 {bail!("agy binary too large for client discovery");}
    let bytes=fs::read(binary)?;
    let wanted_id=if gcp{GCP_ID_HASH}else{CLIENT_ID_HASH};
    let wanted_secrets=if gcp{&[GCP_SECRET_HASH][..]}else{&[CLIENT_SECRET_HASH,CLIENT_SECRET_HASH_V2][..]};
    let suffix=b".apps.googleusercontent.com";
    let mut id=String::new();let mut secret=String::new();
    for i in 0..bytes.len().saturating_sub(suffix.len()) {
        if &bytes[i..i+suffix.len()]!=suffix{continue;}
        let mut start=i;
        while start>0 && i-start<200 && valid_id(bytes[start-1]){start-=1;}
        for s in start..i {
            if i-s+suffix.len()<40{continue;}
            let candidate=&bytes[s..i+suffix.len()];
            if sha(candidate)==wanted_id {id=String::from_utf8_lossy(candidate).into_owned();break;}
        }
        if !id.is_empty(){break;}
    }
    let prefix=b"GOCSPX-";
    for i in 0..bytes.len().saturating_sub(prefix.len()) {
        if &bytes[i..i+prefix.len()]!=prefix {continue;}
        let mut end=i+prefix.len();while end<bytes.len() && end-i<160 && valid_secret(bytes[end]){end+=1;}
        for e in i+20..=end {
            let candidate=&bytes[i..e];
            if wanted_secrets.contains(&sha(candidate).as_str()) {secret=String::from_utf8_lossy(candidate).into_owned();break;}
        }
        if !secret.is_empty(){break;}
    }
    if id.is_empty()||secret.is_empty(){bail!("verified OAuth client not found in installed agy executable; set AGY_AUTH_CLIENT_ID and AGY_AUTH_CLIENT_SECRET or use login --via-agy");}
    Ok((id,secret))
}
fn lookup_bin(bin:&str)->Option<std::path::PathBuf>{
    let p=std::path::PathBuf::from(bin);
    if p.is_file(){return Some(p);}
    env::var_os("PATH").and_then(|paths|env::split_paths(&paths).map(|dir|dir.join(bin)).find(|p|p.is_file()))
}
pub fn client(gcp:bool,agy_bin:&str)->Result<(String,String)>{
    let (key,secret)=if gcp{("AGY_AUTH_GCP_CLIENT_ID","AGY_AUTH_GCP_CLIENT_SECRET")}else{("AGY_AUTH_CLIENT_ID","AGY_AUTH_CLIENT_SECRET")};
    if let (Ok(a),Ok(b))=(env::var(key),env::var(secret)){if !a.is_empty()&&!b.is_empty(){return Ok((a,b));}}
    let path=lookup_bin(if agy_bin.is_empty(){"agy"}else{agy_bin}).ok_or_else(||anyhow!("agy executable not found: pass --agy or set OAuth client environment variables"))?;
    discover_client(&path,gcp)
}
fn agent()->ureq::Agent{ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build()}
fn normalize_code(raw:&str,expected:&str)->Result<String>{
    let value=raw.trim();if value.is_empty(){bail!("empty authorization code");}
    if value.contains("code=")||value.contains("state=") {
        let parsed=url::Url::parse(value).context("paste the full callback URL or bare code")?;
        if parsed.scheme()!="https" || parsed.host_str()!=Some("antigravity.google") || parsed.path()!="/oauth-callback" {bail!("unexpected OAuth redirect URI");}
        let params:std::collections::HashMap<_,_>=parsed.query_pairs().into_owned().collect();
        if params.get("state").map(String::as_str)!=Some(expected){bail!("OAuth state mismatch");}
        if let Some(e)=params.get("error"){bail!("OAuth denied: {e}");}
        return params.get("code").filter(|v|!v.is_empty()).cloned().ok_or_else(||anyhow!("callback URL has no code"));
    }
    Ok(value.into())
}
fn open_url(url:&str)->Result<()> {
    #[cfg(target_os="linux")]let cmd=("xdg-open",vec![url]);
    #[cfg(target_os="macos")]let cmd=("open",vec![url]);
    #[cfg(target_os="windows")]let cmd=("rundll32",vec!["url.dll,FileProtocolHandler",url]);
    #[cfg(not(any(target_os="linux",target_os="macos",target_os="windows")))]return Ok(());
    let result=Command::new(cmd.0).args(cmd.1).spawn()?;drop(result);Ok(())
}
pub fn login(gcp:bool,agy_bin:&str,no_browser:bool)->Result<Credential>{
    let (id,secret)=client(gcp,agy_bin)?;
    let state=random_url(24);let verifier=random_url(32);
    let challenge=URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url=url::Url::parse("https://accounts.google.com/o/oauth2/auth")?;
    url.query_pairs_mut().append_pair("access_type","offline").append_pair("client_id",&id)
        .append_pair("code_challenge",&challenge).append_pair("code_challenge_method","S256")
        .append_pair("prompt","consent").append_pair("redirect_uri",REDIRECT)
        .append_pair("response_type","code").append_pair("scope",if gcp{GCP_SCOPES}else{SCOPES}).append_pair("state",&state);
    println!("Open this URL to sign in:\n{}",url.as_str());
    if !no_browser {if let Err(e)=open_url(url.as_str()){eprintln!("Browser could not open automatically: {e}");}}
    print!("Paste authorization code or callback URL: ");io::stdout().flush()?;
    let mut raw=String::new();io::stdin().read_line(&mut raw)?;
    let code=normalize_code(&raw,&state)?;
    let res=agent().post("https://oauth2.googleapis.com/token").send_form(&[
        ("client_id",id.as_str()),("client_secret",secret.as_str()),("code",code.as_str()),
        ("code_verifier",verifier.as_str()),("grant_type","authorization_code"),("redirect_uri",REDIRECT),
    ]).context("OAuth token exchange failed")?;
    let response:Value=res.into_json()?;
    if response.get("id_token").and_then(Value::as_str).unwrap_or("").is_empty(){bail!("OAuth did not return an id_token");}
    if response.get("refresh_token").and_then(Value::as_str).unwrap_or("").is_empty(){bail!("OAuth did not return a refresh_token; revoke consent and retry");}
    response_credential(&response,if gcp{"gcp"}else{"consumer"})
}
fn response_credential(v:&Value,auth:&str)->Result<Credential>{
    let token=json!({
        "token":{"access_token":v["access_token"],"token_type":v.get("token_type").cloned().unwrap_or_else(||json!("Bearer")),
            "refresh_token":v["refresh_token"],"expiry":(Utc::now()+ChronoDuration::seconds(v["expires_in"].as_i64().unwrap_or(3600))).to_rfc3339()},
        "auth_method":auth,"id_token":v["id_token"]
    });
    Credential::from_bytes(serde_json::to_vec_pretty(&token)?)
}
pub fn refreshed_access_token(credential:&Credential)->Result<String>{
    let refresh=credential.token_field("refresh_token");if refresh.is_empty(){bail!("refresh token missing");}
    let (id,secret)=client(credential.auth_method()=="gcp","")?;
    let res=agent().post("https://oauth2.googleapis.com/token").send_form(&[
        ("client_id",id.as_str()),("client_secret",secret.as_str()),("refresh_token",refresh),("grant_type","refresh_token")
    ])?;
    let data:Value=res.into_json()?;
    data["access_token"].as_str().filter(|s|!s.is_empty()).map(ToOwned::to_owned).ok_or_else(||anyhow!("refreshed access_token missing"))
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn reject_wrong_callback_state(){assert!(normalize_code("https://antigravity.google/oauth-callback?code=x&state=bad","good").is_err());}
    #[test]fn accept_bare_code(){assert_eq!(normalize_code("abc123","state").unwrap(),"abc123");}
}
