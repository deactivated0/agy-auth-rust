use anyhow::{anyhow,bail,Context,Result};
use std::{env,fs,io,path::{Path,PathBuf},process::Command,thread,time::{Duration,Instant}};
use chrono::Utc;
use crate::paths::{Paths,secure_dir};
const FILES:&[&str]=&["Cookies","Cookies-journal","app_storage.json","Preferences"];
const DIRS:&[&str]=&["Local Storage"];
const UPDATER:&str=".updaterId";

pub fn has_profile(root:&Path)->bool{
    FILES.iter().any(|v|root.join(v).is_file())||DIRS.iter().any(|v|root.join(v).is_dir())
}
fn not_symlink(path:&Path)->Result<()> {
    let meta=fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink(){bail!("refusing symlink in Desktop profile: {}",path.display());}
    Ok(())
}
fn copy_tree(src:&Path,dst:&Path)->Result<()> {
    not_symlink(src)?;
    secure_dir(dst)?;
    for ent in fs::read_dir(src)? {
        let entry=ent?;let ftype=entry.file_type()?;
        if ftype.is_symlink(){bail!("Desktop profile contains symlink {}",entry.path().display());}
        let to=dst.join(entry.file_name());
        if ftype.is_dir(){copy_tree(&entry.path(),&to)?;}
        else if ftype.is_file(){copy_file(&entry.path(),&to)?;}
    }Ok(())
}
fn copy_file(from:&Path,to:&Path)->Result<()> {
    not_symlink(from)?;
    if fs::metadata(from)?.len() > (1u64<<30){bail!("Desktop profile file too large: {}",from.display());}
    // Large SQLite files are streamed through a secure temp file instead of accumulating in memory.
    secure_dir(to.parent().ok_or_else(||anyhow!("missing destination directory"))?)?;
    let mut input=fs::File::open(from)?;
    let mut tmp=tempfile::Builder::new().prefix(".agy-auth-rust-").tempfile_in(to.parent().unwrap())?;
    #[cfg(unix)]{use std::os::unix::fs::PermissionsExt;tmp.as_file().set_permissions(fs::Permissions::from_mode(0o600))?;}
    io::copy(&mut input,&mut tmp)?;tmp.as_file().sync_all()?;
    tmp.persist(to).map_err(|e|e.error)?;Ok(())
}
fn copy_profile(src:&Path,dst:&Path,remove_missing:bool)->Result<()> {
    if !has_profile(src){bail!("no Desktop session at {}",src.display());}
    secure_dir(dst)?;
    for name in FILES {
        let from=src.join(name);let to=dst.join(name);
        if from.exists(){copy_file(&from,&to).with_context(||format!("copy {name}"))?;}
        else if remove_missing && to.exists(){fs::remove_file(to)?;}
    }
    for name in DIRS {
        let from=src.join(name);let to=dst.join(name);
        if to.exists(){fs::remove_dir_all(&to)?;}
        if from.exists(){copy_tree(&from,&to)?;}
    }
    let from=src.join(UPDATER);
    if from.exists() {copy_file(&from,&dst.join(UPDATER))?;}
    Ok(())
}
pub fn snapshot(paths:&Paths,id:&str)->Result<()> {
    if running()? {bail!("close Antigravity Desktop before taking a consistent snapshot");}
    let dest=paths.desktop_snapshots.join(id);
    copy_profile(&paths.desktop_live,&dest,true)
}
pub fn preflight_restore(paths:&Paths,id:&str,close_desktop:bool)->Result<()> {
    let src=paths.desktop_snapshots.join(id);
    if !has_profile(&src) {bail!("no Desktop profile stored for {id}");}
    if running()? && !close_desktop {bail!("Antigravity Desktop is running: close it or use --close-desktop");}
    Ok(())
}
pub fn restore(paths:&Paths,id:&str,close_desktop:bool,relaunch:bool)->Result<()> {
    preflight_restore(paths,id,close_desktop)?;
    if running()? {close()?;}
    if has_profile(&paths.desktop_live){
        let stamp=Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();
        copy_profile(&paths.desktop_live,&paths.backups.join("desktop").join(format!("desktop-{stamp}")),false)?;
    }
    copy_profile(&paths.desktop_snapshots.join(id),&paths.desktop_live,true)?;
    if relaunch {launch()?;}
    Ok(())
}
#[cfg(target_os="linux")]
fn pids()->Result<Vec<u32>>{
    let mut out=vec![];
    for entry in fs::read_dir("/proc")? {
        let e=entry?;let Some(s)=e.file_name().to_str().map(ToOwned::to_owned) else {continue};
        let Ok(pid)=s.parse::<u32>() else{continue};
        let comm=fs::read_to_string(e.path().join("comm")).unwrap_or_default();
        if comm.trim().eq_ignore_ascii_case("antigravity") {out.push(pid);}
    }Ok(out)
}
#[cfg(target_os="macos")]
fn pids()->Result<Vec<u32>>{
    let result=Command::new("pgrep").args(["-x","Antigravity"]).output()?;
    Ok(String::from_utf8_lossy(&result.stdout).lines().filter_map(|s|s.parse().ok()).collect())
}
#[cfg(target_os="windows")]
fn pids()->Result<Vec<u32>>{
    let result=Command::new("tasklist").args(["/FI","IMAGENAME eq Antigravity.exe","/FO","CSV","/NH"]).output()?;
    Ok(String::from_utf8_lossy(&result.stdout).lines().filter_map(|l|l.split(',').nth(1).and_then(|s|s.trim_matches('"').parse().ok())).collect())
}
#[cfg(not(any(target_os="linux",target_os="macos",target_os="windows")))]
fn pids()->Result<Vec<u32>>{Ok(vec![])}
pub fn running()->Result<bool>{Ok(!pids()?.is_empty())}
fn close()->Result<()> {
    let found=pids()?;
    for pid in &found {
        #[cfg(unix)] {let result=Command::new("kill").args(["-TERM",&pid.to_string()]).status()?;if !result.success(){bail!("could not terminate Desktop PID {pid}");}}
        #[cfg(windows)] {let result=Command::new("taskkill").args(["/PID",&pid.to_string()]).status()?;if !result.success(){bail!("could not terminate Desktop PID {pid}");}}
    }
    let start=Instant::now();while running()? {if start.elapsed()>Duration::from_secs(5){bail!("Desktop did not exit after SIGTERM");}thread::sleep(Duration::from_millis(150));}
    Ok(())
}
fn binary()->PathBuf{
    if let Some(v)=env::var_os("AGY_DESKTOP_BIN"){return PathBuf::from(v);}
    #[cfg(target_os="windows")]{return PathBuf::from(env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Programs/Antigravity/Antigravity.exe");}
    #[cfg(target_os="macos")]{return PathBuf::from("/Applications/Antigravity.app/Contents/MacOS/Antigravity");}
    #[cfg(target_os="linux")]{
        for p in ["/opt/antigravity/Antigravity-x64/antigravity","/opt/antigravity/antigravity","/usr/bin/antigravity","/usr/local/bin/antigravity"] {if Path::new(p).exists(){return PathBuf::from(p);}}
    }
    PathBuf::from("antigravity")
}
fn launch()->Result<()> {
    Command::new(binary()).spawn().context("failed to relaunch Antigravity Desktop")?;Ok(())
}
