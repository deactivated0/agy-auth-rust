//! Import user-supplied credentials at runtime. No actual user data is embedded in the project.
//! Accepts native agy JSON, original agy-auth AES-GCM snapshot files, standard
//! OAuth token JSON, and explicitly wrapped arrays of either format.
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{Map, Value};
use std::path::Path;
use crate::{crypto, model::Credential};

const MAX_ENTRIES: usize = 1024;

pub fn decode(bytes: &[u8], key_file: &Path) -> Result<Vec<Credential>> {
    // Decryption needs the *user's own* encryption.key on their machine.
    let plain = crypto::decrypt(key_file, bytes)?;
    let root: Value = serde_json::from_slice(&plain).context("credential input must be JSON or an original encrypted snapshot")?;
    // Preserve the exact source document for native input to avoid changing
    // unknown fields, token expiry representations, or formatting.
    if root.is_object() && root.get("token").is_some() {
        return Ok(vec![Credential::from_bytes(plain)?]);
    }
    let mut extracted = Vec::new();
    expand(root, &mut extracted)?;
    if extracted.is_empty() { bail!("no credential records found (registry.json only stores metadata)"); }
    if extracted.len() > MAX_ENTRIES { bail!("too many credential records"); }
    Ok(extracted)
}

fn expand(value: Value, result: &mut Vec<Credential>) -> Result<()> {
    if result.len() >= MAX_ENTRIES { bail!("too many credential records (limit {MAX_ENTRIES})"); }
    match value {
        Value::Array(values) => {
            for value in values { expand(value, result)?; }
        }
        Value::Object(mut map) => {
            if map.contains_key("token") {
                // Preserve exactly the same native format and unknown fields.
                result.push(Credential::from_bytes(serde_json::to_vec(&Value::Object(map))?)?);
            } else if map.contains_key("access_token") || map.contains_key("refresh_token") {
                result.push(Credential::from_bytes(serde_json::to_vec(&normalize_oauth(map))?)?);
            } else if let Some(inner) = map.remove("credential").or_else(|| map.remove("credentials")) {
                expand(inner, result)?;
            } else if let Some(inner) = map.remove("accounts") {
                // Reject registry.json metadata rather than guessing it has tokens.
                if map.contains_key("version") { bail!("this is a registry index, not an account export; use 'import --purge' with the encrypted snapshots"); }
                expand(inner, result)?;
            } else if let Some(inner) = map.remove("auth") {
                expand(inner, result)?;
            } else {
                return Err(anyhow!("unrecognized credential format; no tokens or supported wrapper found"));
            }
        }
        _ => bail!("credential input must be a JSON object or array"),
    }
    Ok(())
}

fn normalize_oauth(mut raw: Map<String,Value>) -> Value {
    let mut token = Map::new();
    for name in ["access_token", "refresh_token", "token_type", "expiry"] {
        if let Some(value) = raw.remove(name) { token.insert(name.to_string(), value); }
    }
    if !token.contains_key("expiry") {
        // Standard OAuth exports may have expires_at instead of expiry.
        if let Some(value) = raw.remove("expires_at") { token.insert("expiry".into(), value); }
    }
    raw.insert("token".into(), Value::Object(token));
    raw.entry("auth_method").or_insert(Value::String("consumer".into()));
    Value::Object(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_credential_keeps_original_bytes() {
        let raw = br#"{ "token": {"refresh_token": "synthetic-example"}, "custom": true }"#;
        let records = decode(raw,Path::new("/nonexistent/key")).unwrap();
        assert_eq!(records[0].raw, raw);
    }
    #[test]
    fn oauth_export_normalizes_and_keeps_refresh() {
        let raw = br#"{"access_token":"synthetic-access","refresh_token":"synthetic-refresh","id_token_email":"example.invalid"}"#;
        let records = decode(raw,Path::new("/nonexistent/key")).unwrap();
        assert_eq!(records[0].token_field("refresh_token"),"synthetic-refresh");
    }
    #[test]
    fn encrypted_original_format_is_accepted_with_external_key() {
        let tmp=tempfile::tempdir().unwrap();
        let key=tmp.path().join("encryption.key");
        let synthetic=br#"{"token":{"refresh_token":"synthetic-not-real"}}"#;
        let encrypted=crypto::encrypt(&key,synthetic).unwrap();
        let parsed=decode(&encrypted,&key).unwrap();
        assert_eq!(parsed[0].token_field("refresh_token"),"synthetic-not-real");
    }
    #[test]
    fn bundled_list_is_imported_without_disk_secrets() {
        let raw=br#"{"credentials":[{"token":{"refresh_token":"synthetic-one"}},{"token":{"refresh_token":"synthetic-two"}}]}"#;
        assert_eq!(decode(raw,Path::new("/nonexistent/key")).unwrap().len(),2);
    }
    #[test]
    fn registry_metadata_is_not_a_credential() {
        let raw=br#"{"version":2,"accounts":[{"id":"acct_0123456789","email":"safe.invalid"}]}"#;
        assert!(decode(raw,Path::new("/nonexistent/key")).is_err());
    }
}
