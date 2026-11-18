use anyhow::{anyhow,bail,Result};
use chrono::{DateTime,Utc};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::{HashMap,VecDeque},sync::{mpsc,Arc,Mutex},thread,time::Duration};
use crate::{model::{Account,Credential},oauth,store::Store};

const API:&str="https://cloudcode-pa.googleapis.com/v1internal";
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Bucket {pub id:String,pub name:String,pub window:String,pub remaining_percent:u8,pub reset_time:Option<String>}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Group {pub name:String,pub description:String,pub buckets:Vec<Bucket>}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Usage {pub account:String,pub plan:String,pub five_hour:Option<Bucket>,pub weekly:Option<Bucket>,pub groups:Vec<Group>,pub updated_at:DateTime<Utc>}
impl Usage {
    pub fn min_remaining(&self)->Option<u8>{self.five_hour.as_ref().map(|b|b.remaining_percent).into_iter().chain(self.weekly.as_ref().map(|b|b.remaining_percent)).min()}
    pub fn exhausted(&self)->bool{self.groups.iter().flat_map(|g|&g.buckets).any(|b|b.remaining_percent==0)}
}
fn project(credential:&Credential)->String{
    if !credential.field("project_id").is_empty(){return credential.field("project_id").into();}
    for key in ["AGY_AUTH_PROJECT_ID","ANTIGRAVITY_PROJECT_ID","GEMINI_DEFAULT_PROJECT"]{
        if let Ok(v)=std::env::var(key){if !v.trim().is_empty(){return v;}}
    }
    if let Some(home)=dirs::home_dir(){if let Ok(s)=std::fs::read_to_string(home.join(".gemini/antigravity-cli/cache/default_project_id.txt")){if !s.trim().is_empty(){return s.trim().into();}}}
    "default-cli-project".into()
}
fn plan(tier:&Value,credential:&Credential)->String {
    for candidate in [&tier["paidTier"],&tier["currentTier"]] {
        if candidate.is_null(){continue;}
        let id=candidate["id"].as_str().unwrap_or("");
        let label=match id{"free-tier"=>"Free","standard-tier"=>"Standard","g1-pro-tier"=>"Pro","g1-ultra-tier"=>"Ultra",_=>""};
        if !label.is_empty(){return label.into();}
        if let Some(name)=candidate["name"].as_str(){if !name.is_empty(){return name.into();}}
        if !id.is_empty(){return id.into();}
    }
    if credential.auth_method()=="gcp"{"PayGo".into()}else{String::new()}
}
fn post(ag:&ureq::Agent,method:&str,token:&str,body:&Value)->Result<Value>{
    let url=format!("{API}:{method}");
    let ua=format!("antigravity/cli/1.4.5 (agy-auth-rust/{}; os_type={}; arch={}; auth_method=consumer)",env!("CARGO_PKG_VERSION"),std::env::consts::OS,std::env::consts::ARCH);
    let response=ag.post(&url).set("Authorization",&format!("Bearer {token}")).set("User-Agent",&ua).set("Accept","application/json").send_json(body.clone());
    let response=response.map_err(|e|anyhow!("quota {method}: {e}"))?;
    if response.status()>=300{bail!("quota {method} failed (HTTP {})",response.status());}
    Ok(response.into_json()?)
}
fn bucket_window(window:&str,id:&str)->Option<&'static str>{
    let w=window.to_ascii_lowercase();
    match w.as_str(){"5h"|"5-hour"|"five_hour"|"five-hour"=>Some("5h"),"weekly"|"week"=>Some("weekly"),_=>{
        let id=id.to_ascii_lowercase();if id.contains("week"){Some("weekly")}else if id.contains("5h")||id.contains("five"){Some("5h")}else{None}
    }}
}
fn lower(target:&mut Option<Bucket>,candidate:&Bucket){
    let replace=match target{
        None=>true,
        Some(current)=>candidate.remaining_percent<current.remaining_percent ||
            (candidate.remaining_percent==current.remaining_percent && candidate.reset_time.is_some() && (current.reset_time.is_none()||candidate.reset_time<current.reset_time))
    };
    if replace{*target=Some(candidate.clone());}
}
pub fn fetch(c:&Credential)->Result<Usage>{
    let expired=chrono::DateTime::parse_from_rfc3339(c.token_field("expiry")).map(|t|t.with_timezone(&Utc)<Utc::now()+chrono::Duration::seconds(30)).unwrap_or(false);
    let access=if c.token_field("access_token").is_empty()||expired{oauth::refreshed_access_token(c)?}else{c.token_field("access_token").to_owned()};
    let project=project(c);
    let ag=ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).build();
    let assist=post(&ag,"loadCodeAssist",&access,&json!({"cloudaicompanionProject":project,"metadata":{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI","duetProject":project}}))?;
    let mut usage=Usage{account:c.email.clone(),plan:plan(&assist,c),five_hour:None,weekly:None,groups:vec![],updated_at:Utc::now()};
    if let Ok(summary)=post(&ag,"retrieveUserQuotaSummary",&access,&json!({"project":project})){
        if let Some(sources)=summary["groups"].as_array(){
            for g in sources {
                let mut group=Group{name:g["displayName"].as_str().unwrap_or("Unnamed group").into(),description:g["description"].as_str().unwrap_or("").into(),buckets:vec![]};
                if let Some(buckets)=g["buckets"].as_array(){for b in buckets {
                    let Some(f)=b["remainingFraction"].as_f64()else{continue};
                    if !f.is_finite(){continue;}
                    let id=b["bucketId"].as_str().unwrap_or("").to_owned();
                    let window=b["window"].as_str().unwrap_or("").to_owned();
                    let bucket=Bucket{id:id.clone(),name:b["displayName"].as_str().unwrap_or("").into(),window:window.clone(),remaining_percent:(f.clamp(0.,1.)*100.).round() as u8,reset_time:b["resetTime"].as_str().map(ToOwned::to_owned)};
                    match bucket_window(&window,&id){Some("5h")=>lower(&mut usage.five_hour,&bucket),Some("weekly")=>lower(&mut usage.weekly,&bucket),_=>()}
                    group.buckets.push(bucket);
                }}
                usage.groups.push(group);
            }
        }
    }
    Ok(usage)
}

/// Fetch quotas concurrently without spawning an unbounded thread per account.
pub fn fetch_all(store:&Store,accounts:&[Account])->Vec<(String,Result<Usage,String>)>{
    let work:VecDeque<_>=accounts.iter().map(|a|a.id.clone()).collect();
    let queue=Arc::new(Mutex::new(work));
    let (tx,rx)=mpsc::channel();
    thread::scope(|scope|{
        for _ in 0..accounts.len().min(4) {
            let queue=Arc::clone(&queue);let tx=tx.clone();
            scope.spawn(move||loop {
                let id=queue.lock().expect("queue mutex poisoned").pop_front();
                let Some(id)=id else{break};
                let output=store.read_snapshot(&id).and_then(|c|fetch(&c)).map_err(|e|e.to_string());
                if tx.send((id,output)).is_err(){break;}
            });
        }
        drop(tx);
    });
    let entries:HashMap<_,_>=rx.into_iter().collect();
    accounts.iter().map(|a|(a.id.clone(),entries.get(&a.id).map(|r|match r{Ok(u)=>Ok(u.clone()),Err(e)=>Err(e.clone())}).unwrap_or_else(||Err("quota result missing".into())))).collect()
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn choose_lowest(){let b=|n|Bucket{id:"5h".into(),name:"".into(),window:"5h".into(),remaining_percent:n,reset_time:None};let mut a=None;lower(&mut a,&b(50));lower(&mut a,&b(75));lower(&mut a,&b(1));assert_eq!(a.unwrap().remaining_percent,1);}
}
