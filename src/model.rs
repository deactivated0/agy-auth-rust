use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

fn is_false(value: &bool) -> bool { !*value }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
    #[serde(default, rename = "sub", skip_serializing_if = "String::is_empty")]
    pub subject: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alias: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub auth_method: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub project_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub region: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_cli: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_desktop: bool,
    pub added_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_5h: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_weekly: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_reset_5h: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_reset_weekly: Option<String>,
}
impl Default for Account {
    fn default()->Self {Self{id:String::new(),email:String::new(),subject:String::new(),alias:String::new(),auth_method:String::new(),project_id:String::new(),region:String::new(),fingerprint:String::new(),has_cli:false,has_desktop:false,added_at:Utc::now(),last_used_at:None,plan:None,quota_5h:None,quota_weekly:None,quota_reset_5h:None,quota_reset_weekly:None}}
}
impl Account {
    pub fn display(&self) -> String {
        let base = match (self.alias.is_empty(), self.email.is_empty()) {
            (false, false) => format!("{} ({})", self.alias, self.email),
            (false, true) => self.alias.clone(),
            (true, false) => self.email.clone(),
            (true, true) => self.id.clone(),
        };
        if self.project_id.is_empty() { base } else { format!("{base} [{}]", self.project_id) }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_interval_seconds: Option<u64>,
    #[serde(default)]
    pub snapshot_encryption: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Registry {
    pub version: u8,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub active: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub previous: String,
    #[serde(default)]
    pub config: Config,
    #[serde(default)]
    pub accounts: Vec<Account>,
}
impl Default for Registry {
    fn default() -> Self { Self { version: 2, active: String::new(), previous: String::new(), config: Config { live_interval_seconds: None, snapshot_encryption: "aes-gcm-v1".into() }, accounts: vec![] } }
}
pub fn plan_rank(plan: Option<&str>) -> u8 {
    match plan.unwrap_or("").to_lowercase().as_str() {
        p if p.contains("ultra") => 0,
        p if p.contains("pro") => 1,
        p if p.contains("plus") || p.contains("premium") => 2,
        p if p.contains("standard") => 3,
        p if p.contains("paygo") => 4,
        p if p.contains("free") => 10,
        "" => 15,
        _ => 12,
    }
}

impl Registry {
    pub fn normalize(&mut self) -> Result<()> {
        if self.version > 2 { bail!("registry v{} is newer than supported v2", self.version); }
        self.version = 2;
        if self.config.snapshot_encryption.is_empty() { self.config.snapshot_encryption = "aes-gcm-v1".into(); }
        let mut ids = std::collections::HashSet::new();
        let mut aliases = std::collections::HashSet::new();
        for account in &self.accounts {
            validate_id(&account.id)?;
            if !ids.insert(account.id.clone()) { bail!("duplicate account id {}", account.id); }
            if !account.alias.is_empty() && !aliases.insert(account.alias.to_ascii_lowercase()) { bail!("duplicate account alias {}", account.alias); }
        }
        if !ids.contains(&self.active) { self.active.clear(); }
        if !ids.contains(&self.previous) || self.active == self.previous { self.previous.clear(); }
        self.accounts.sort_by(|a,b| {
            (a.id != self.active).cmp(&(b.id != self.active))
                .then_with(|| plan_rank(a.plan.as_deref()).cmp(&plan_rank(b.plan.as_deref())))
                .then_with(|| a.email.is_empty().cmp(&b.email.is_empty()))
                .then_with(|| a.email.to_ascii_lowercase().cmp(&b.email.to_ascii_lowercase()))
                .then_with(|| a.alias.to_ascii_lowercase().cmp(&b.alias.to_ascii_lowercase()))
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(())
    }
    pub fn index(&self, query: &str) -> Result<usize> {
        let query=query.trim();
        if query.is_empty(){bail!("account query is empty");}
        if query == "-" { return self.accounts.iter().position(|a| a.id == self.previous).ok_or_else(|| anyhow!("no previous account")); }
        if let Ok(n) = query.parse::<usize>() {if n >= 1 && n <= self.accounts.len() {return Ok(n-1);}else{bail!("row number out of range");}}
        for (i,a) in self.accounts.iter().enumerate(){if a.id.eq_ignore_ascii_case(query){return Ok(i);}}
        for (i,a) in self.accounts.iter().enumerate(){if !a.alias.is_empty()&&a.alias.eq_ignore_ascii_case(query){return Ok(i);}}
        let matches=self.accounts.iter().enumerate().filter_map(|(i,a)|
            ((!a.project_id.is_empty()&&a.project_id.eq_ignore_ascii_case(query)) || (!a.email.is_empty()&&a.email.eq_ignore_ascii_case(query))).then_some(i)
        ).collect::<Vec<_>>();
        if matches.len()==1{return Ok(matches[0]);}
        if matches.len()>1{bail!("ambiguous account query {query:?}");}
        let q=query.to_lowercase();
        let partial=self.accounts.iter().enumerate().filter_map(|(i,a)|
            (a.id.to_lowercase().starts_with(&q) || a.email.to_lowercase().contains(&q) || a.alias.to_lowercase().contains(&q) || a.project_id.to_lowercase().contains(&q)).then_some(i)
        ).collect::<Vec<_>>();
        match partial.as_slice(){[i]=>Ok(*i),[]=>bail!("no account matches {query:?}"),_=>bail!("ambiguous account query {query:?}")}
    }
    pub fn activate(&mut self, id: &str) -> Result<()> {
        if !self.accounts.iter().any(|a| a.id == id) { bail!("account not found: {id}"); }
        if self.active != id { self.previous = self.active.clone(); self.active = id.to_owned(); }
        if self.previous == self.active { self.previous.clear(); }
        if let Some(a) = self.accounts.iter_mut().find(|a| a.id == id) { a.last_used_at = Some(Utc::now()); }
        self.normalize()
    }
}

pub fn validate_id(id: &str) -> Result<()> {
    if id.len() < 6 || id.len() > 85 || !id.starts_with("acct_") || !id[5..].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' ) {
        bail!("invalid account id {id:?}");
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct Credential {
    pub raw: Vec<u8>,
    pub doc: Value,
    pub subject: String,
    pub email: String,
    pub fingerprint: String,
}
impl Credential {
    pub fn from_bytes(raw: Vec<u8>) -> Result<Self> {
        let doc: Value = serde_json::from_slice(&raw)?;
        let tok = doc.get("token").ok_or_else(|| anyhow!("missing token object"))?;
        let access = tok.get("access_token").and_then(Value::as_str).unwrap_or("");
        let refresh = tok.get("refresh_token").and_then(Value::as_str).unwrap_or("");
        if access.is_empty() && refresh.is_empty() { bail!("credential has no access or refresh token"); }
        let claims = doc.get("id_token").and_then(Value::as_str).and_then(|id| id.split('.').nth(1)).and_then(|part| URL_SAFE_NO_PAD.decode(part).ok()).and_then(|raw| serde_json::from_slice::<Value>(&raw).ok()).unwrap_or(Value::Null);
        let subject = claims.get("sub").and_then(Value::as_str).unwrap_or("").to_owned();
        let email = claims.get("email").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).or_else(|| doc.get("id_token_email").and_then(Value::as_str)).unwrap_or("").trim().to_owned();
        let fingerprint = format!("{:x}", Sha256::digest(if !refresh.is_empty() {refresh.as_bytes()} else {access.as_bytes()}));
        Ok(Self { raw, doc, subject, email, fingerprint: fingerprint[..16].to_owned() })
    }
    pub fn field(&self, name: &str) -> &str { self.doc.get(name).and_then(Value::as_str).unwrap_or("") }
    pub fn token_field(&self, name: &str) -> &str { self.doc.get("token").and_then(|t| t.get(name)).and_then(Value::as_str).unwrap_or("") }
    pub fn auth_method(&self) -> &str { if self.field("auth_method").is_empty() { "consumer" } else { self.field("auth_method") } }
    pub fn id(&self) -> String {
        let base = if self.subject.is_empty() { &self.fingerprint } else { &self.subject };
        let mut key=base.to_owned();
        if self.auth_method().eq_ignore_ascii_case("gcp") && !self.field("project_id").is_empty() {
            key.push_str(":gcp:");key.push_str(&self.field("project_id").trim().to_ascii_lowercase());
        }
        let digest = format!("{:x}", Sha256::digest(key.as_bytes()));
        format!("acct_{}", &digest[..10])
    }
    pub fn expired_without_refresh(&self) -> bool {
        let expiry = self.token_field("expiry");
        let expired = chrono::DateTime::parse_from_rfc3339(expiry).map(|t| t.with_timezone(&Utc) <= Utc::now()).unwrap_or(false);
        expired && self.token_field("refresh_token").is_empty()
    }
    pub fn with_gcp(mut self, project: &str, region: &str) -> Result<Self> {
        if project.trim().is_empty() || project.contains(|ch:char| ch.is_whitespace() || ch == '/') { bail!("invalid project id"); }
        if !["global", "us", "eu"].contains(&region) { bail!("region must be global, us or eu"); }
        self.doc["auth_method"] = Value::String("gcp".into());
        self.doc["project_id"] = Value::String(project.to_owned());
        self.doc["region"] = Value::String(region.to_owned());
        if let Some(obj) = self.doc.as_object_mut() { obj.remove("user_tier"); obj.remove("tier_display_name"); }
        self.raw = serde_json::to_vec_pretty(&self.doc)?;
        Ok(self)
    }
    pub fn account(&self, alias: &str) -> Account {
        Account { id: self.id(), email: self.email.clone(), subject: self.subject.clone(), alias: alias.into(), auth_method: self.auth_method().into(), project_id: self.field("project_id").into(), region: self.field("region").into(), fingerprint: self.fingerprint.clone(), has_cli: true, has_desktop: false, added_at: Utc::now(), last_used_at: None, plan: None, quota_5h: None, quota_weekly: None, quota_reset_5h: None, quota_reset_weekly: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_subject_same_id_after_token_rotation() {
        let mk = |token: &str| Credential::from_bytes(format!(r#"{{"token":{{"access_token":"{token}"}},"id_token":"e30.eyJzdWIiOiJzdWItMSIsImVtYWlsIjoiYWJjQGV4YW1wbGUub3JnIn0.x"}}"#).into_bytes()).unwrap();
        assert_eq!(mk("A").id(), mk("B").id());
    }
    #[test]
    fn ambiguous_alias_rejected() {
        let mut r = Registry::default();
        let a = Account { id:"acct_1234567890".into(), alias:"Test".into(), added_at:Utc::now(), ..Default::default() };
        let b = Account { id:"acct_abcdefghij".into(), alias:"test".into(), added_at:Utc::now(), ..Default::default() };
        r.accounts = vec![a,b];
        assert!(r.normalize().is_err());
    }
}
