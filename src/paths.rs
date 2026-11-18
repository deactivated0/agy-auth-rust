use anyhow::{anyhow, Context, Result};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Paths {
    pub base: PathBuf,
    pub accounts: PathBuf,
    pub desktop_snapshots: PathBuf,
    pub backups: PathBuf,
    pub live_token: PathBuf,
    pub live_settings: PathBuf,
    pub installation_id: PathBuf,
    pub desktop_live: PathBuf,
}
impl Paths {
    pub fn discover() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("HOME directory not found"))?;
        let base = env::var_os("AGY_AUTH_RUST_HOME")
            .or_else(|| env::var_os("AGYBUFF_AUTH_HOME"))
            .or_else(|| env::var_os("AGY_AUTH_HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::config_dir()
                    .unwrap_or_else(|| home.join(".config"))
                    .join("agy-auth")
            });
        let cli_dir = home.join(".gemini/antigravity-cli");
        let live_token = env::var_os("AGY_AUTH_AGY_TOKEN_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| cli_dir.join("antigravity-oauth-token"));
        let live_settings = env::var_os("AGY_AUTH_AGY_SETTINGS_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                live_token
                    .parent()
                    .unwrap_or(&cli_dir)
                    .join("settings.json")
            });
        let installation_id = live_token
            .parent()
            .unwrap_or(&cli_dir)
            .join("installation_id");
        let desktop_live = env::var_os("AGY_DESKTOP_HOME")
            .or_else(|| env::var_os("AGY_DESKTOP_CONFIG_DIR"))
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::config_dir()
                    .unwrap_or_else(|| home.join(".config"))
                    .join("Antigravity")
            });
        Ok(Self {
            accounts: base.join("accounts"),
            desktop_snapshots: base.join("desktop"),
            backups: base.join("backups"),
            base,
            live_token,
            live_settings,
            installation_id,
            desktop_live,
        })
    }
    pub fn registry(&self) -> PathBuf {
        self.base.join("registry.json")
    }
    pub fn key(&self) -> PathBuf {
        self.base.join("encryption.key")
    }
    pub fn snapshot(&self, id: &str) -> PathBuf {
        self.accounts.join(format!("{id}.json"))
    }
    pub fn install_snapshot(&self, id: &str) -> PathBuf {
        self.accounts.join(format!("{id}.installation_id"))
    }
}

pub fn secure_dir(path: &Path) -> Result<()> {
    // Create newly managed directories with 0700, without chmodding arbitrary
    // existing parent directories supplied by the user.
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder
            .create(path)
            .with_context(|| format!("create {}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    }
    Ok(())
}
pub fn read_limit(path: &Path, max: u64) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| format!("read {}", path.display()))?;
    if file.metadata()?.len() > max {
        return Err(anyhow!("file too large: {}", path.display()));
    }
    let mut bytes = Vec::with_capacity(file.metadata()?.len() as usize);
    file.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(anyhow!("file too large: {}", path.display()));
    }
    Ok(bytes)
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("path missing parent"))?;
    secure_dir(dir)?;
    let mut temp = tempfile::Builder::new()
        .prefix(".agy-auth-rust-")
        .tempfile_in(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    {
        let _ = File::open(dir).and_then(|f| f.sync_all());
    }
    Ok(())
}

#[allow(clippy::suspicious_open_options)]
pub fn exclusive_file(path: &Path) -> Result<File> {
    secure_dir(path.parent().ok_or_else(|| anyhow!("lock lacks parent"))?)?;
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    fs2::FileExt::lock_exclusive(&file)?;
    Ok(file)
}

pub fn bytes_match(path: &Path, bytes: &[u8]) -> bool {
    fs::metadata(path)
        .ok()
        .is_some_and(|m| m.len() == bytes.len() as u64)
        && fs::read(path).map(|v| v == bytes).unwrap_or(false)
}

pub fn safe_component(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .take(60)
        .collect();
    if cleaned.is_empty() {
        "account".into()
    } else {
        cleaned
    }
}
