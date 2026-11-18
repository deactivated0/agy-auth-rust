use anyhow::{anyhow,bail,Context,Result};
use chrono::Utc;
use serde_json::Value;
use std::{env,fs,path::Path};
use uuid::Uuid;
use crate::{crypto,model::{Account,Credential,Registry,validate_id},paths::{Paths,atomic_write,read_limit,bytes_match,exclusive_file,secure_dir}};

pub struct Store {pub paths:Paths}
impl Store {
    pub fn new(paths:Paths)->Self{Self{paths}}
    pub fn discover()->Result<Self>{Ok(Self::new(Paths::discover()?))}
    pub fn mutation_guard(&self)->Result<std::fs::File>{exclusive_file(&self.paths.base.join(".registry.lock"))}
    pub fn load(&self)->Result<Registry>{
        let path=self.paths.registry();
        if !path.exists(){return Ok(Registry::default());}
        let mut r:Registry=serde_json::from_slice(&read_limit(&path,8<<20)?).context("invalid registry.json")?;
        r.normalize()?; Ok(r)
    }
    pub fn save(&self,reg:&Registry)->Result<()> {
        let mut normalized=reg.clone();normalized.normalize()?;
        let mut bytes=serde_json::to_vec_pretty(&normalized)?;bytes.push(b'\n');
        let path=self.paths.registry();
        if bytes_match(&path,&bytes){return Ok(());}
        if path.exists(){
            let existing=read_limit(&path,8<<20)?;
            self.backup("registry",&existing)?;
        }
        atomic_write(&path,&bytes)
    }
    fn backup(&self,kind:&str,bytes:&[u8])->Result<()> {
        secure_dir(&self.paths.backups)?;
        let stem=format!("{kind}-{}",Utc::now().format("%Y%m%dT%H%M%S%.9fZ"));
        let mut path=self.paths.backups.join(format!("{stem}.json"));
        let mut suffix=0usize;
        while path.exists(){suffix+=1;path=self.paths.backups.join(format!("{stem}-{suffix}.json"));}
        atomic_write(&path,bytes)?;
        self.prune_backups(false)?;
        Ok(())
    }
    fn managed_backup(name:&str,kind:&str)->bool {
        let Some(rest)=name.strip_prefix(&format!("{kind}-"))else{return false};
        let Some(rest)=rest.strip_suffix(".json")else{return false};
        if rest.len()<26{return false;}
        let b=rest.as_bytes();
        if b[8]!=b'T'||b[15]!=b'.'||b[25]!=b'Z'{return false;}
        for (i,v) in b[..26].iter().enumerate(){if i==8||i==15||i==25{continue;}if !v.is_ascii_digit(){return false;}}
        rest.len()==26 || (rest.len()>27 && b[26]==b'-' && b[27..].iter().all(|c|c.is_ascii_digit()))
    }
    pub fn prune_backups(&self,dry_run:bool)->Result<Vec<String>> {
        let mut removed=vec![];
        if !self.paths.backups.exists(){return Ok(removed);}
        for kind in ["active","registry"]{
            let mut files=fs::read_dir(&self.paths.backups)?.filter_map(|e|e.ok()).filter(|e|e.file_type().map(|t|t.is_file()).unwrap_or(false)).filter(|e|Self::managed_backup(&e.file_name().to_string_lossy(),kind)).collect::<Vec<_>>();
            files.sort_by_key(|e|std::cmp::Reverse(e.file_name()));
            for file in files.into_iter().skip(5){
                let name=file.file_name().to_string_lossy().into_owned();
                if !dry_run {fs::remove_file(file.path())?;}
                removed.push(name);
            }
        }
        removed.sort();Ok(removed)
    }
    pub fn read_snapshot(&self,id:&str)->Result<Credential>{
        validate_id(id)?;
        let raw=read_limit(&self.paths.snapshot(id),4<<20)?;
        Credential::from_bytes(crypto::decrypt(&self.paths.key(),&raw)?)
    }
    pub fn save_snapshot(&self,id:&str,credential:&Credential)->Result<()> {
        validate_id(id)?;secure_dir(&self.paths.accounts)?;
        let path=self.paths.snapshot(id);
        if let Ok(existing)=self.read_snapshot(id){
            if existing.raw==credential.raw && fs::read(&path).map(|b|crypto::encrypted(&b)).unwrap_or(false){return Ok(());}
        }
        let sealed=crypto::encrypt(&self.paths.key(),&credential.raw)?;
        atomic_write(&path,&sealed)
    }
    pub fn read_live(&self)->Result<Credential>{
        if env::var("AGY_AUTH_STORAGE").as_deref()!=Ok("file") {
            if let Ok(entry)=keyring::Entry::new("gemini","antigravity") {
                if let Ok(value)=entry.get_password(){
                    if let Ok(creds)=Credential::from_bytes(value.into_bytes()){return Ok(creds);}
                }
            }
        }
        Credential::from_bytes(read_limit(&self.paths.live_token,4<<20)?).context("missing or invalid live agy credential")
    }
    pub fn write_live(&self,credential:&Credential)->Result<Vec<String>>{
        if credential.expired_without_refresh(){bail!("access token expired and refresh token is missing");}
        if let Ok(existing)=self.read_live(){
            if existing.raw!=credential.raw {
                self.backup("active",&crypto::encrypt(&self.paths.key(),&existing.raw)?)?;
            }
        }
        // Always persist the fallback even when the system keyring is unavailable.
        // A keyring-only write could otherwise leave an old file that agy uses in headless sessions.
        atomic_write(&self.paths.live_token,&credential.raw)?;
        let mut warnings=vec![];
        if env::var("AGY_AUTH_STORAGE").as_deref()!=Ok("file") {
            match keyring::Entry::new("gemini","antigravity") {
                Ok(entry)=>if let Err(e)=entry.set_password(std::str::from_utf8(&credential.raw)?){warnings.push(format!("keyring write failed, file fallback is current: {e}"));},
                Err(e)=>warnings.push(format!("keyring unavailable, file fallback is current: {e}")),
            }
        }
        self.sync_settings(credential)?;
        Ok(warnings)
    }
    pub fn sync_settings(&self,credential:&Credential)->Result<()> {
        let path=&self.paths.live_settings;
        if !path.exists() && credential.auth_method()!="gcp" {return Ok(());}
        let mut settings:Value=if path.exists(){serde_json::from_slice(&read_limit(path,4<<20)?)?}else{serde_json::json!({})};
        let obj=settings.as_object_mut().ok_or_else(||anyhow!("settings.json must contain a JSON object"))?;
        if credential.auth_method()=="gcp" || !credential.field("project_id").is_empty(){
            let project=credential.field("project_id");
            if project.is_empty(){bail!("GCP credential is missing project_id");}
            let region=if credential.field("region").is_empty(){"global"}else{credential.field("region")};
            let mut gcp=obj.remove("gcp").unwrap_or_else(||serde_json::json!({}));
            if !gcp.is_object(){gcp=serde_json::json!({});}
            gcp["project"]=Value::String(project.to_owned());gcp["location"]=Value::String(region.to_owned());
            obj.insert("gcp".into(),gcp);
        }else if obj.remove("gcp").is_none(){return Ok(());}
        let mut bytes=serde_json::to_vec_pretty(&settings)?;bytes.push(b'\n');
        if !bytes_match(path,&bytes){atomic_write(path,&bytes)?;}
        Ok(())
    }
    pub fn upsert(&self,reg:&mut Registry,credential:&Credential,alias:&str)->Result<(Account,bool)> {
        let mut new=credential.account(alias);
        validate_id(&new.id)?;
        // Match the Go store's ID, then subject, then refresh-token fingerprint
        // within the same consumer/GCP scope. Preserve legacy IDs on refresh.
        let scope=|a:&Account|{
            let a_gcp=a.auth_method.eq_ignore_ascii_case("gcp") || !a.project_id.is_empty();
            let new_gcp=new.auth_method.eq_ignore_ascii_case("gcp") || !new.project_id.is_empty();
            a_gcp==new_gcp && (!a_gcp || a.project_id.eq_ignore_ascii_case(&new.project_id))
        };
        if let Some(existing)=reg.accounts.iter().find(|a| a.id==new.id ||
            (scope(a) && ((!new.subject.is_empty() && a.subject==new.subject) ||
            (!new.fingerprint.is_empty() && a.fingerprint==new.fingerprint)))) {
            new.id=existing.id.clone();
        }
        if !alias.is_empty() && (alias.contains(char::is_whitespace) || reg.accounts.iter().any(|a|a.id!=new.id && a.alias.eq_ignore_ascii_case(alias))){bail!("alias must be unique and contain no whitespace");}
        self.save_snapshot(&new.id,credential)?;
        let inserted=if let Some(old)=reg.accounts.iter_mut().find(|a|a.id==new.id){
            old.email=new.email.clone();old.subject=new.subject.clone();old.fingerprint=new.fingerprint.clone();old.auth_method=new.auth_method.clone();old.project_id=new.project_id.clone();old.region=new.region.clone();old.has_cli=true;
            if !alias.is_empty(){old.alias=alias.into();}false
        }else{reg.accounts.push(new.clone());true};
        reg.normalize()?;
        let id_path=self.paths.install_snapshot(&new.id);
        if !id_path.exists(){
            // A shared live installation id may only belong to the currently active credential.
            let live_match=self.read_live().ok().is_some_and(|c|c.fingerprint==credential.fingerprint && c.field("project_id")==credential.field("project_id"));
            let id=if live_match {fs::read_to_string(&self.paths.installation_id).ok().map(|s|s.trim().to_owned()).filter(|s|Uuid::parse_str(s).is_ok())}else{None};
            let used=|v:&str|reg.accounts.iter().filter(|a|a.id!=new.id).any(|a|fs::read_to_string(self.paths.install_snapshot(&a.id)).map(|s|s.trim()==v).unwrap_or(false));
            let value=id.filter(|v|!used(v)).unwrap_or_else(||Uuid::new_v4().to_string());
            atomic_write(&id_path,value.as_bytes())?;
        }
        Ok((reg.accounts.iter().find(|a|a.id==new.id).ok_or_else(||anyhow!("account missing"))?.clone(),inserted))
    }
    pub fn switch(&self,reg:&mut Registry,id:&str,update_cli:bool,update_desktop:bool,close_desktop:bool,relaunch:bool)->Result<Vec<String>> {
        validate_id(id)?;
        let account=reg.accounts.iter().find(|a|a.id==id).ok_or_else(||anyhow!("account not found"))?.clone();
        // Preflight all selected sources before changing either live target.
        let credential=if update_cli{Some(self.read_snapshot(id)?)}else{None};
        if update_desktop { crate::desktop::preflight_restore(&self.paths,id,close_desktop)?; }
        if let Some(c)=credential.as_ref(){if c.expired_without_refresh(){bail!("expired credential without refresh token");}}
        let mut messages=vec![];
        if let Some(c)=credential.as_ref(){
            messages.extend(self.write_live(c)?);
            let id_path=self.paths.install_snapshot(id);
            let id_value=fs::read_to_string(&id_path).ok().filter(|v|Uuid::parse_str(v.trim()).is_ok()).unwrap_or_else(||Uuid::new_v4().to_string());
            atomic_write(&id_path,id_value.trim().as_bytes())?;
            atomic_write(&self.paths.installation_id,id_value.trim().as_bytes())?;
        }
        if update_desktop {crate::desktop::restore(&self.paths,id,close_desktop,relaunch)?;}
        reg.activate(&account.id)?;
        self.save(reg)?;
        Ok(messages)
    }
    pub fn remove(&self,reg:&mut Registry,ids:&[String],logout:bool)->Result<()> {
        let removed:std::collections::HashSet<String>=ids.iter().cloned().collect();
        let was_active=removed.contains(&reg.active);
        reg.accounts.retain(|a|!removed.contains(&a.id));
        if removed.contains(&reg.active){reg.active.clear();}
        if removed.contains(&reg.previous){reg.previous.clear();}
        self.save(reg)?;
        // Delete after saving registry: failure can leave recoverable orphan snapshots, not lost registry entries.
        for id in ids {
            validate_id(id)?;
            for path in [self.paths.snapshot(id),self.paths.install_snapshot(id)]{
                if let Err(e)=fs::remove_file(&path){if e.kind()!=std::io::ErrorKind::NotFound{return Err(e).with_context(||format!("delete {}",path.display()));}}
            }
            let _=fs::remove_dir_all(self.paths.desktop_snapshots.join(id));
        }
        if logout && was_active {
            match fs::remove_file(&self.paths.live_token){Ok(())=>(),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>(),Err(e)=>return Err(e.into())}
            if env::var("AGY_AUTH_STORAGE").as_deref()!=Ok("file") {
                if let Ok(entry)=keyring::Entry::new("gemini","antigravity"){let _=entry.delete_password();}
            }
        }
        Ok(())
    }
    pub fn purge(&self,old:&Registry)->Result<(Registry,usize)> {
        let mut r=Registry::default();r.config=old.config.clone();
        if !self.paths.accounts.exists(){return Ok((r,0));}
        for file in fs::read_dir(&self.paths.accounts)? {
            let entry=file?;let name=entry.file_name().to_string_lossy().into_owned();
            let Some(id)=name.strip_suffix(".json") else{continue};
            if validate_id(id).is_err(){continue;}
            let Ok(c)=self.read_snapshot(id)else{continue};
            let Some(previous)=old.accounts.iter().find(|a|a.id==id ||(!a.subject.is_empty() && a.subject==c.subject)) else{
                let mut a=c.account("");a.id=id.into();r.accounts.push(a);continue;
            };
            let mut a=c.account(&previous.alias);a.id=id.into();a.added_at=previous.added_at;a.last_used_at=previous.last_used_at;a.has_desktop=previous.has_desktop;r.accounts.push(a);
        }
        if r.accounts.iter().any(|a|a.id==old.active){r.active=old.active.clone();}
        if r.accounts.iter().any(|a|a.id==old.previous){r.previous=old.previous.clone();}
        r.normalize()?;
        let n=r.accounts.len();Ok((r,n))
    }
    pub fn clean(&self,dry_run:bool)->Result<Vec<String>>{
        let r=self.load()?;let mut removed=vec![];
        removed.extend(self.prune_backups(dry_run)?);
        if !self.paths.accounts.exists()||!self.paths.registry().exists(){return Ok(removed);}
        for file in fs::read_dir(&self.paths.accounts)?{
            let entry=file?;if !entry.file_type()?.is_file(){continue;}
            let name=entry.file_name().to_string_lossy().to_string();
            let id=name.strip_suffix(".json").or_else(||name.strip_suffix(".installation_id"));
            if let Some(id)=id {if validate_id(id).is_ok() && !r.accounts.iter().any(|a|a.id==id){
                removed.push(name);if !dry_run{fs::remove_file(entry.path())?;}
            }}
        }
        Ok(removed)
    }
    pub fn export(&self,reg:&Registry,ids:&[String],to:&Path)->Result<usize>{
        secure_dir(to)?;let mut count=0;
        for id in ids {
            let c=self.read_snapshot(id)?;
            let a=reg.accounts.iter().find(|a|a.id==*id).ok_or_else(||anyhow!("unknown account"))?;
            let safe=crate::paths::safe_component(if a.alias.is_empty(){&a.email}else{&a.alias});
            let short=id.trim_start_matches("acct_").chars().take(10).collect::<String>();
            let p=to.join(format!("{safe}-{short}.json"));
            // No silent overwrite: exported files are plaintext credentials.
            if p.exists(){bail!("refusing to overwrite existing plaintext export {}",p.display());}
            atomic_write(&p,&c.raw)?;count+=1;
        }Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_store()->(tempfile::TempDir,Store){
        let dir=tempfile::tempdir().unwrap();
        let base=dir.path().join("state");let cli=dir.path().join("agy");
        let paths=Paths{base:base.clone(),accounts:base.join("accounts"),desktop_snapshots:base.join("desktop"),backups:base.join("backups"),live_token:cli.join("token"),live_settings:cli.join("settings.json"),installation_id:cli.join("installation_id"),desktop_live:dir.path().join("desktop-live")};
        (dir,Store::new(paths))
    }
    #[test]fn encrypted_stored_import_roundtrip(){
        let (_t,s)=test_store();let mut r=s.load().unwrap();
        let c=Credential::from_bytes(br#"{"token":{"access_token":"hello","refresh_token":"secret"},"auth_method":"consumer"}"#.to_vec()).unwrap();
        let (a,_)=s.upsert(&mut r,&c,"personal").unwrap();s.save(&r).unwrap();
        assert!(crypto::encrypted(&fs::read(s.paths.snapshot(&a.id)).unwrap()));
        assert_eq!(s.read_snapshot(&a.id).unwrap().raw,c.raw);
        assert_eq!(s.load().unwrap().accounts[0].alias,"personal");
    }
}
