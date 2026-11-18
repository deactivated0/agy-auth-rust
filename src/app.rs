use crate::{
    desktop, formats,
    model::{Account, Credential, Registry},
    oauth, paths, quota,
    store::Store,
    tui::{self, Mode},
};
use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    env, fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Parser, Debug)]
#[command(name="agy-auth-rust",version,about="Fast Rust Antigravity multi-account manager",long_about=None)]
pub struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    no_color: bool,
    #[command(subcommand)]
    command: CommandLine,
}
#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum Target {
    All,
    Cli,
    Desktop,
}
#[derive(Args, Clone, Debug, Default)]
pub struct GcpFlags {
    #[arg(long)]
    gcp: bool,
    #[arg(long)]
    project: Option<String>,
    #[arg(long, default_value = "global")]
    region: String,
}
#[derive(Subcommand, Debug)]
enum CommandLine {
    #[command(alias = "ls")]
    List {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        active: bool,
        #[arg(long)]
        no_sync: bool,
        #[arg(long)]
        no_usage: bool,
        #[arg(long)]
        live: bool,
    },
    Add {
        #[arg(long)]
        alias: Option<String>,
        #[arg(long,value_enum,default_value_t=Target::All)]
        target: Target,
        #[command(flatten)]
        gcp: GcpFlags,
    },
    Login {
        #[arg(long)]
        alias: Option<String>,
        #[arg(long)]
        via_agy: bool,
        #[arg(long)]
        no_browser: bool,
        #[arg(long)]
        agy: Option<String>,
        #[arg(long)]
        auth_method: Option<String>,
        #[command(flatten)]
        gcp: GcpFlags,
    },
    #[command(alias = "use")]
    Switch {
        query: Option<String>,
        #[arg(long)]
        live: bool,
        #[arg(long,value_enum,default_value_t=Target::All)]
        target: Target,
        #[arg(long)]
        close_desktop: bool,
        #[arg(long)]
        relaunch: bool,
    },
    Rotate {
        #[arg(long,value_enum,default_value_t=Target::All)]
        target: Target,
        #[arg(long)]
        close_desktop: bool,
        #[arg(long)]
        relaunch: bool,
        #[arg(long)]
        force: bool,
    },
    #[command(alias = "rm")]
    Remove {
        query: Vec<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        logout: bool,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        live: bool,
    },
    Alias {
        #[command(subcommand)]
        cmd: AliasCmd,
    },
    Current,
    Status,
    Usage {
        query: Option<String>,
        #[arg(long)]
        all: bool,
    },
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    Import {
        path: Option<PathBuf>,
        #[arg(long)]
        alias: Option<String>,
        #[command(flatten)]
        gcp: GcpFlags,
        #[arg(long)]
        activate: bool,
        #[arg(long)]
        purge: bool,
        #[arg(long)]
        key: Option<PathBuf>,
    },
    Export {
        query: Vec<String>,
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    Path,
    Clean {
        #[arg(long)]
        dry_run: bool,
    },
    Version {
        #[arg(long)]
        check: bool,
    },
    Sort,
}
#[derive(Subcommand, Debug)]
enum AliasCmd {
    Set { query: String, alias: String },
    Clear { query: String },
    List,
}
#[derive(Subcommand, Debug)]
enum ConfigCmd {
    Live {
        #[arg(long)]
        interval: Option<u64>,
    },
}

fn output(v: Value, verbose: impl FnOnce(), json_output: bool) -> Result<()> {
    if json_output {
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        verbose();
    }
    Ok(())
}
fn print_accounts(
    reg: &Registry,
    accounts: &[Account],
    usage: &[(String, Result<quota::Usage, String>)],
) {
    println!(
        "{:>3}  {:<45} {:<10} {:<8} {:<8}",
        "#", "ACCOUNT", "PLAN", "5H", "WEEKLY"
    );
    for (i, a) in accounts.iter().enumerate() {
        let u = usage.iter().find(|(id, _)| id == &a.id);
        let (plan, five, week) = match u {
            Some((_, Ok(u))) => (
                u.plan.clone(),
                u.five_hour
                    .as_ref()
                    .map(|b| {
                        if (b.remaining_percent - 100.0).abs() < 0.05 {
                            "100%".into()
                        } else {
                            format!("{:.1}%", b.remaining_percent)
                        }
                    })
                    .unwrap_or_else(|| "-".into()),
                u.weekly
                    .as_ref()
                    .map(|b| {
                        if (b.remaining_percent - 100.0).abs() < 0.05 {
                            "100%".into()
                        } else {
                            format!("{:.1}%", b.remaining_percent)
                        }
                    })
                    .unwrap_or_else(|| "-".into()),
            ),
            Some((_, Err(_))) => ("?".into(), "?".into(), "?".into()),
            None => (
                a.plan.clone().unwrap_or_else(|| "-".into()),
                "-".into(),
                "-".into(),
            ),
        };
        let active = if a.id == reg.active { "*" } else { " " };
        let display = a.display();
        let mut chars = display.chars();
        let short: String = chars.by_ref().take(44).collect();
        println!(
            "{:>2}{}  {:<45} {:<10} {:<8} {:<8}",
            i + 1,
            active,
            short,
            plan,
            five,
            week
        );
    }
}
#[allow(dead_code)]
fn choose(reg: &Registry, query: Option<&str>, mode: Mode) -> Result<Option<String>> {
    if let Some(q) = query {
        return Ok(Some(reg.accounts[reg.index(q)?].id.clone()));
    }
    let picks = tui::interact(&reg.accounts, &reg.active, mode)?;
    Ok(picks.first().map(|i| reg.accounts[*i].id.clone()))
}
fn confirm(prompt: &str) -> Result<bool> {
    if !io::stdin().is_terminal() {
        bail!("confirmation requires a terminal; pass --yes");
    }
    print!("{prompt} [y/N]: ");
    io::stdout().flush()?;
    let mut s = String::new();
    io::stdin().read_line(&mut s)?;
    Ok(matches!(s.trim().to_lowercase().as_str(), "y" | "yes"))
}
fn validate_alias(reg: &Registry, id: &str, alias: &str) -> Result<()> {
    if alias.is_empty() || alias.contains(char::is_whitespace) {
        bail!("alias must be nonempty and contain no whitespace");
    }
    if reg
        .accounts
        .iter()
        .any(|a| a.id != id && a.alias.eq_ignore_ascii_case(alias))
    {
        bail!("alias already in use");
    }
    Ok(())
}
fn target_flags(t: Target, store: &Store, id: &str) -> Result<(bool, bool)> {
    let cli = t != Target::Desktop;
    let desktop = t != Target::Cli && desktop::has_profile(&store.paths.desktop_snapshots.join(id));
    if t == Target::Desktop && !desktop {
        bail!("no Desktop profile stored for this account");
    }
    Ok((cli, desktop))
}
pub fn activate(
    store: &Store,
    reg: &mut Registry,
    id: &str,
    target: Target,
    close: bool,
    relaunch: bool,
) -> Result<Vec<String>> {
    let (cli, desktop) = target_flags(target, store, id)?;
    store.switch(reg, id, cli, desktop, close, relaunch)
}
fn read_alias(v: &Option<String>) -> &str {
    v.as_deref().unwrap_or("")
}
fn is_gcp(gcp: &GcpFlags) -> bool {
    gcp.gcp || gcp.project.is_some()
}
fn as_gcp(c: Credential, gcp: &GcpFlags) -> Result<Credential> {
    if is_gcp(gcp) {
        c.with_gcp(
            gcp.project
                .as_deref()
                .ok_or_else(|| anyhow!("--gcp requires --project"))?,
            &gcp.region,
        )
    } else {
        Ok(c)
    }
}
pub fn import_one(
    store: &Store,
    reg: &mut Registry,
    path: &Path,
    alias: &str,
    gcp: &GcpFlags,
    key: &Path,
) -> Result<Vec<String>> {
    let records = formats::decode(&paths::read_limit(path, 4 << 20)?, key)?;
    if records.len() != 1 && !alias.is_empty() {
        bail!("--alias is only valid for single-credential input");
    }
    let mut ids = Vec::with_capacity(records.len());
    for credential in records {
        let credential = as_gcp(credential, gcp)?;
        let (a, _) = store.upsert(reg, &credential, alias)?;
        ids.push(a.id);
    }
    Ok(ids)
}
fn import_via_agy(bin: &str) -> Result<Credential> {
    let temp = tempfile::tempdir()?;
    let home = temp.path();
    let agy = home.join(".gemini/antigravity-cli");
    paths::secure_dir(&agy)?;
    println!("Opening official agy CLI with isolated HOME; complete sign-in there.");
    let binpath = if bin.is_empty() { "agy" } else { bin };
    let mut process = Command::new(binpath);
    process
        .current_dir(home)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"));
    #[cfg(windows)]
    {
        process
            .env("USERPROFILE", home)
            .env("APPDATA", home.join("AppData/Roaming"))
            .env("LOCALAPPDATA", home.join("AppData/Local"));
    }
    let result = process
        .status()
        .context("failed to run agy for isolated login")?;
    let path = agy.join("antigravity-oauth-token");
    if !path.exists() {
        bail!("agy did not create credentials in isolated HOME (exit status {result})");
    }
    Credential::from_bytes(paths::read_limit(&path, 4 << 20)?)
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let store = Store::discover()?;
    let json_flag = cli.json;
    match cli.command {
        CommandLine::Path => {
            output(
                json!({"home":store.paths.base,"registry":store.paths.registry(),"accounts":store.paths.accounts,"encryption_key":store.paths.key(),"live_token":store.paths.live_token,"desktop":store.paths.desktop_live}),
                || {
                    println!(
                        "Registry: {}\nSnapshots: {}\nKey: {}\nLive CLI: {}\nDesktop: {}",
                        store.paths.registry().display(),
                        store.paths.accounts.display(),
                        store.paths.key().display(),
                        store.paths.live_token.display(),
                        store.paths.desktop_live.display()
                    );
                },
                json_flag,
            )?;
        }
        CommandLine::Version { check } => {
            if check {
                eprintln!("Automatic update checks are not implemented in this version; review the source repository's releases.");
            }
            output(
                json!({"name":"agy-auth-rust","version":env!("CARGO_PKG_VERSION")}),
                || println!("agy-auth-rust {}", env!("CARGO_PKG_VERSION")),
                json_flag,
            )?;
        }
        CommandLine::Current => {
            let live = store.read_live()?;
            let r = store.load()?;
            let found = r.accounts.iter().find(|a| {
                (!live.subject.is_empty()
                    && a.subject == live.subject
                    && a.project_id == live.field("project_id"))
                    || (a.fingerprint == live.fingerprint
                        && a.project_id == live.field("project_id"))
            });
            let label = found.map(|a| a.display()).unwrap_or_else(|| {
                if live.email.is_empty() {
                    "Unregistered account".into()
                } else {
                    live.email.clone()
                }
            });
            output(
                json!({"email":live.email,"id":found.map(|a|a.id.as_str()),"name":label,"auth_method":live.auth_method()}),
                || println!("{label}"),
                json_flag,
            )?;
        }
        CommandLine::Status => {
            let r = store.load()?;
            let live = store.read_live();
            let running = desktop::running().unwrap_or(false);
            let v = json!({"accounts":r.accounts.len(),"active":r.active,"previous":r.previous,"live_credential_valid":live.is_ok(),"live_email":live.as_ref().map(|c|c.email.as_str()).unwrap_or(""),"desktop_running":running,"registry":store.paths.registry(),"encrypted_snapshots":true});
            output(
                v,
                || {
                    println!("Stored accounts: {}\nActive: {}\nLive CLI credential: {}\nDesktop running: {}\nRegistry: {}",r.accounts.len(),r.active,if live.is_ok(){"valid"}else{"missing or invalid"},running,store.paths.registry().display())
                },
                json_flag,
            )?;
        }
        CommandLine::List {
            all,
            active,
            no_sync,
            no_usage,
            live,
        } => {
            if !no_sync {
                if let Ok(livecred) = store.read_live() {
                    let _lock = store.mutation_guard()?;
                    let mut r = store.load()?;
                    if let Some(a) = r
                        .accounts
                        .iter()
                        .find(|a| {
                            (a.fingerprint == livecred.fingerprint
                                && a.project_id == livecred.field("project_id"))
                                || (!livecred.subject.is_empty()
                                    && a.subject == livecred.subject
                                    && a.project_id == livecred.field("project_id"))
                        })
                        .cloned()
                    {
                        let _ = store.save_snapshot(&a.id, &livecred);
                        if r.active != a.id {
                            r.activate(&a.id)?;
                            store.save(&r)?;
                        }
                    }
                }
            }
            let r = store.load()?;
            if live {
                tui::interact(&r.accounts, &r.active, Mode::List)?;
                return Ok(());
            }
            let accounts: Vec<_> = r
                .accounts
                .iter()
                .filter(|a| !active || a.id == r.active)
                .cloned()
                .collect();
            let usage = if no_usage {
                vec![]
            } else {
                quota::fetch_all(&store, &accounts)
            };
            output(
                json!({"accounts":accounts,"active":r.active,"usage":usage.iter().map(|(id,u)|json!({"id":id,"result":u.as_ref().ok(),"error":u.as_ref().err()})).collect::<Vec<_>>()}),
                || {
                    print_accounts(&r, &accounts, &usage);
                    if all {
                        for (_, v) in &usage {
                            if let Ok(u) = v {
                                print_usage(u);
                            }
                        }
                    }
                },
                json_flag,
            )?;
            if !no_usage && !usage.is_empty() {
                if let Ok(mut r_mut) = store.load() {
                    let mut changed = false;
                    for a in &mut r_mut.accounts {
                        if let Some((_, Ok(u))) = usage.iter().find(|(uid, _)| uid == &a.id) {
                            if !u.plan.is_empty() {
                                a.plan = Some(u.plan.clone());
                            }
                            a.quota_5h = u.five_hour.as_ref().map(|b| b.remaining_percent);
                            a.quota_weekly = u.weekly.as_ref().map(|b| b.remaining_percent);
                            a.quota_reset_5h =
                                u.five_hour.as_ref().and_then(|b| b.reset_time.clone());
                            a.quota_reset_weekly =
                                u.weekly.as_ref().and_then(|b| b.reset_time.clone());
                            a.quota_group_desc = u.gemini_group().map(|g| g.description.clone());
                            changed = true;
                        }
                    }
                    if changed {
                        let _ = store.save(&r_mut);
                    }
                }
            }
        }
        CommandLine::Add { alias, target, gcp } => {
            let _lock = store.mutation_guard()?;
            let mut r = store.load()?;
            // A desktop-only add still requires the live CLI credential for identity.
            let cred = as_gcp(store.read_live()?, &gcp)?;
            let (a, inserted) = store.upsert(&mut r, &cred, read_alias(&alias))?;
            if target != Target::Cli {
                if desktop::has_profile(&store.paths.desktop_live) && !desktop::running()? {
                    desktop::snapshot(&store.paths, &a.id)?;
                    if let Some(item) = r.accounts.iter_mut().find(|v| v.id == a.id) {
                        item.has_desktop = true;
                    }
                } else if target == Target::Desktop {
                    bail!(
                        "no stopped Desktop profile available; close Antigravity Desktop and retry"
                    );
                } else {
                    eprintln!("warning: Desktop profile not captured (missing or running); CLI credential was stored");
                }
            }
            if r.active.is_empty() {
                r.activate(&a.id)?;
            }
            store.save(&r)?;
            output(
                json!({"account":a,"added":inserted}),
                || {
                    println!(
                        "Saved {} ({}).",
                        a.display(),
                        if inserted { "new" } else { "updated" }
                    )
                },
                json_flag,
            )?;
        }
        CommandLine::Login {
            alias,
            via_agy,
            no_browser,
            agy,
            auth_method,
            gcp,
        } => {
            if let Some(ref auth) = auth_method {
                if auth != "consumer" && auth != "gcp" {
                    bail!("unsupported auth method");
                }
            }
            let method = auth_method.as_deref() == Some("gcp") || is_gcp(&gcp);
            let cred = if via_agy {
                import_via_agy(agy.as_deref().unwrap_or(""))?
            } else {
                oauth::login(method, agy.as_deref().unwrap_or(""), no_browser)?
            };
            let cred = as_gcp(cred, &gcp)?;
            let _lock = store.mutation_guard()?;
            let mut r = store.load()?;
            if let Ok(previous) = store.read_live() {
                let _ = store.upsert(&mut r, &previous, "");
            }
            let (a, _) = store.upsert(&mut r, &cred, read_alias(&alias))?;
            store.switch(&mut r, &a.id, true, false, false, false)?;
            output(
                json!({"account":a,"active":r.active}),
                || println!("Signed in and activated {}.", a.display()),
                json_flag,
            )?;
        }
        CommandLine::Switch {
            query,
            live: _,
            target,
            close_desktop,
            relaunch,
        } => {
            let _lock = store.mutation_guard()?;
            let mut r = store.load()?;
            if let Some(ref q) = query {
                let id = r.accounts[r.index(q)?].id.clone();
                let messages = activate(&store, &mut r, &id, target, close_desktop, relaunch)?;
                output(
                    json!({"active":id,"target":format!("{target:?}").to_lowercase(),"warnings":messages}),
                    || {
                        println!(
                            "Switched to {}. Restart running agy sessions.",
                            r.accounts
                                .iter()
                                .find(|a| a.id == id)
                                .map(|a| a.display())
                                .unwrap_or(id.clone())
                        );
                        for msg in messages {
                            eprintln!("warning: {msg}");
                        }
                    },
                    json_flag,
                )?;
            } else {
                tui::dashboard(&store, &mut r, target, close_desktop, relaunch)?;
            }
        }
        CommandLine::Rotate {
            target,
            close_desktop,
            relaunch,
            force,
        } => {
            let _lock = store.mutation_guard()?;
            let mut r = store.load()?;
            if r.accounts.len() < 2 {
                bail!("rotation requires at least two stored accounts");
            }
            let results = quota::fetch_all(&store, &r.accounts);
            let active = results
                .iter()
                .find(|(id, _)| id == &r.active)
                .and_then(|(_, u)| u.as_ref().ok());
            if !force {
                let active = active.ok_or_else(|| {
                    anyhow!("could not verify active-account quota; refusing automatic rotation")
                })?;
                if !active.exhausted() {
                    return output(
                        json!({"switched":false,"active":r.active,"reason":"quota_available"}),
                        || println!("Current account still has quota. No rotation needed."),
                        json_flag,
                    );
                }
            }
            let best = results
                .iter()
                .filter(|(id, _)| id != &r.active)
                .filter_map(|(id, u)| {
                    u.as_ref().ok().and_then(|u| {
                        let remaining = u.min_remaining().unwrap_or(0.0);
                        (remaining > 0.0 && !u.exhausted())
                            .then_some((id.clone(), (remaining * 100.0) as i64))
                    })
                })
                .max_by_key(|(_, v)| *v);
            let Some((id, _)) = best else {
                bail!("no verified healthy alternative account; rotation canceled");
            };
            let warnings = activate(&store, &mut r, &id, target, close_desktop, relaunch)?;
            output(
                json!({"switched":true,"active":id,"warnings":warnings}),
                || println!("Rotated to {id}."),
                json_flag,
            )?;
        }
        CommandLine::Remove {
            query,
            all,
            logout,
            yes,
            live,
        } => {
            let _lock = store.mutation_guard()?;
            let mut r = store.load()?;
            let ids: Vec<String> = if all {
                r.accounts.iter().map(|a| a.id.clone()).collect()
            } else if !query.is_empty() {
                query
                    .iter()
                    .map(|q| r.index(q).map(|i| r.accounts[i].id.clone()))
                    .collect::<Result<_>>()?
            } else if live || io::stdin().is_terminal() {
                tui::interact(&r.accounts, &r.active, Mode::Remove)?
                    .into_iter()
                    .map(|i| r.accounts[i].id.clone())
                    .collect()
            } else {
                bail!("provide an account query or --all");
            };
            let unique: Vec<_> = ids
                .into_iter()
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            if unique.is_empty() {
                return Ok(());
            }
            if !yes && !confirm(&format!("Remove {} account(s)?", unique.len()))? {
                return Ok(());
            }
            store.remove(&mut r, &unique, logout)?;
            output(
                json!({"removed":unique,"logged_out":logout}),
                || println!("Removed {} account(s).", unique.len()),
                json_flag,
            )?;
        }
        CommandLine::Alias { cmd } => {
            let _lock = store.mutation_guard()?;
            let mut r = store.load()?;
            match cmd {
                AliasCmd::List => {
                    output(
                        json!({"aliases":r.accounts.iter().filter(|a|!a.alias.is_empty()).map(|a|json!({"id":a.id,"alias":a.alias})).collect::<Vec<_>>()}),
                        || {
                            for a in &r.accounts {
                                if !a.alias.is_empty() {
                                    println!("{}  {}", a.id, a.alias);
                                }
                            }
                        },
                        json_flag,
                    )?;
                }
                AliasCmd::Set { query, alias } => {
                    let i = r.index(&query)?;
                    let id = r.accounts[i].id.clone();
                    validate_alias(&r, &id, &alias)?;
                    r.accounts[i].alias = alias.clone();
                    store.save(&r)?;
                    output(
                        json!({"id":id,"alias":alias}),
                        || println!("Alias set: {alias}"),
                        json_flag,
                    )?;
                }
                AliasCmd::Clear { query } => {
                    let i = r.index(&query)?;
                    r.accounts[i].alias.clear();
                    store.save(&r)?;
                    output(
                        json!({"id":r.accounts[i].id}),
                        || println!("Alias cleared."),
                        json_flag,
                    )?;
                }
            }
        }
        CommandLine::Usage { query, all } => {
            let r = store.load()?;
            let accounts = if all {
                r.accounts.clone()
            } else {
                vec![if let Some(ref q) = query {
                    r.accounts[r.index(q)?].clone()
                } else {
                    r.accounts
                        .iter()
                        .find(|a| a.id == r.active)
                        .ok_or_else(|| anyhow!("no active account"))?
                        .clone()
                }]
            };
            let usage = quota::fetch_all(&store, &accounts);
            output(
                json!({"usage":usage.iter().map(|(id,u)|json!({"id":id,"result":u.as_ref().ok(),"error":u.as_ref().err()})).collect::<Vec<_>>()}),
                || {
                    for (id, result) in usage {
                        match result {
                            Ok(u) => print_usage(&u),
                            Err(e) => eprintln!("{id}: {e}"),
                        }
                    }
                },
                json_flag,
            )?;
        }
        CommandLine::Import {
            path,
            alias,
            gcp,
            activate: do_activate,
            purge,
            key,
        } => {
            let _lock = store.mutation_guard()?;
            let mut r = store.load()?;
            if purge {
                let (restored, count) = store.purge(&r)?;
                store.save(&restored)?;
                output(
                    json!({"purged":true,"accounts":count}),
                    || println!("Rebuilt registry from {count} snapshots."),
                    json_flag,
                )?;
            } else {
                let path =
                    path.ok_or_else(|| anyhow!("import requires a file or directory, or --purge"))?;
                let files: Vec<_> = if path.is_dir() {
                    let mut v = fs::read_dir(&path)?
                        .filter_map(|e| e.ok().map(|e| e.path()))
                        .filter(|p| p.extension().is_some_and(|ext| ext == "json") && p.is_file())
                        .collect::<Vec<_>>();
                    v.sort();
                    v
                } else {
                    vec![path]
                };
                if files.len() > 1 && alias.is_some() {
                    bail!("--alias cannot be used when importing multiple files");
                }
                let mut imported = vec![];
                let mut failed = vec![];
                let key_path = key.unwrap_or_else(|| store.paths.key());
                for file in &files {
                    match import_one(&store, &mut r, file, read_alias(&alias), &gcp, &key_path) {
                        Ok(ids) => imported.extend(ids),
                        Err(e) => failed.push(format!("{}: {e}", file.display())),
                    }
                }
                if do_activate && imported.len() != 1 {
                    bail!("--activate requires exactly one successful import");
                }
                if do_activate {
                    store.switch(&mut r, &imported[0], true, false, false, false)?;
                } else {
                    store.save(&r)?;
                }
                output(
                    json!({"imported":imported,"errors":failed}),
                    || {
                        println!(
                            "Imported {} account(s), {} failed.",
                            imported.len(),
                            failed.len()
                        );
                        for e in &failed {
                            eprintln!("{e}");
                        }
                    },
                    json_flag,
                )?;
                if !failed.is_empty() {
                    bail!("one or more credentials could not be imported");
                }
            }
        }
        CommandLine::Export { query, dir } => {
            let r = store.load()?;
            let ids: Vec<_> = if query.is_empty() {
                r.accounts.iter().map(|a| a.id.clone()).collect()
            } else {
                query
                    .iter()
                    .map(|q| r.index(q).map(|i| r.accounts[i].id.clone()))
                    .collect::<Result<_>>()?
            };
            let dest = dir.unwrap_or_else(|| PathBuf::from("."));
            let count = store.export(&r, &ids, &dest)?;
            output(
                json!({"exported":count,"directory":dest}),
                || {
                    println!("Exported {count} plaintext credential(s) to {}. Protect and delete them after use.",dest.display())
                },
                json_flag,
            )?;
        }
        CommandLine::Clean { dry_run } => {
            let _lock = store.mutation_guard()?;
            let names = store.clean(dry_run)?;
            output(
                json!({"dry_run":dry_run,"files":names}),
                || {
                    for name in &names {
                        println!(
                            "{} {name}",
                            if dry_run { "Would remove" } else { "Removed" }
                        );
                    }
                    println!(
                        "{} snapshot(s) {}.",
                        names.len(),
                        if dry_run { "identified" } else { "cleaned" }
                    );
                },
                json_flag,
            )?;
        }
        CommandLine::Config { cmd } => {
            let _lock = store.mutation_guard()?;
            let mut r = store.load()?;
            match cmd {
                ConfigCmd::Live { interval } => {
                    if let Some(seconds) = interval {
                        if !(5..=3600).contains(&seconds) {
                            bail!("live interval must be 5 through 3600 seconds");
                        }
                        r.config.live_interval_seconds = Some(seconds);
                        store.save(&r)?;
                    }
                    let val = r.config.live_interval_seconds.unwrap_or(60);
                    output(
                        json!({"live_interval_seconds":val}),
                        || println!("Live refresh interval: {val}s"),
                        json_flag,
                    )?;
                }
            }
        }
        CommandLine::Sort => {
            let _lock = store.mutation_guard()?;
            let mut r = store.load()?;
            let usage = quota::fetch_all(&store, &r.accounts);
            for a in &mut r.accounts {
                if let Some((_, Ok(u))) = usage.iter().find(|(uid, _)| uid == &a.id) {
                    if !u.plan.is_empty() {
                        a.plan = Some(u.plan.clone());
                    }
                    a.quota_5h = u.five_hour.as_ref().map(|b| b.remaining_percent);
                    a.quota_weekly = u.weekly.as_ref().map(|b| b.remaining_percent);
                    a.quota_reset_5h = u.five_hour.as_ref().and_then(|b| b.reset_time.clone());
                    a.quota_reset_weekly = u.weekly.as_ref().and_then(|b| b.reset_time.clone());
                    a.quota_group_desc = u.gemini_group().map(|g| g.description.clone());
                }
            }
            r.normalize()?;
            store.save(&r)?;
            output(
                json!({"accounts":r.accounts,"active":r.active}),
                || {
                    println!("Resorted accounts (premium accounts at the top):\n");
                    print_accounts(&r, &r.accounts, &usage);
                },
                json_flag,
            )?;
        }
    }
    Ok(())
}
fn print_usage(u: &quota::Usage) {
    println!("{} | {} | {}", u.account, u.plan, u.updated_at);
    for g in &u.groups {
        println!("  {}", g.name);
        if !g.description.is_empty() {
            println!("    {}", g.description);
        }
        for b in &g.buckets {
            let reset_str = b
                .reset_time
                .as_deref()
                .map(|rt| format!("{} ({})", tui::format_relative_countdown(rt), rt))
                .unwrap_or_default();
            println!(
                "    {:<12} {:>6.2}%  {}",
                b.window, b.remaining_percent, reset_str
            );
        }
    }
}

pub fn run_switcher() -> Result<()> {
    let store = Store::discover()?;
    let _lock = store.mutation_guard()?;
    let mut r = store.load()?;
    if r.accounts.is_empty() {
        bail!("no registered accounts");
    }
    tui::dashboard(&store, &mut r, Target::All, false, false)?;
    Ok(())
}
