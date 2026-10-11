//! Configuration: environment loading and `.env` persistence.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

pub const AUTHORIZE_URL: &str = "https://www.donationalerts.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://www.donationalerts.com/oauth/token";
pub const API_BASE: &str = "https://www.donationalerts.com/api/v1";
pub const WS_URL: &str = "wss://centrifugo.donationalerts.com/connection/websocket";

pub const DEFAULT_REDIRECT_URI: &str = "https://gunlinux.ru/callback/";
pub const DEFAULT_SCOPES: &str = "oauth-user-show oauth-donation-subscribe oauth-donation-index";

const ACCESS_TOKEN_KEY: &str = "ACCESS_TOKEN";

/// Runtime configuration assembled from `.env` and the process environment.
pub struct Config {
    pub app_id: String,
    pub client_secret: Option<String>,
    pub user_id: Option<String>,
    pub access_token: Option<String>,
    pub redirect_uri: String,
    pub scopes: String,
    env_path: PathBuf,
}

impl Config {
    /// Load the process environment, overlaying `.env` when it is present.
    pub fn load() -> Result<Self> {
        let env_path = PathBuf::from(".env");
        let _ = dotenvy::from_filename(&env_path); // a missing .env is fine

        let app_id = std::env::var("APP_ID").unwrap_or_default();
        if app_id.is_empty() {
            bail!("APP_ID is not set (put it in .env or the environment)");
        }

        Ok(Self {
            app_id,
            client_secret: non_empty(std::env::var("CLIENT_SECRET").ok()),
            user_id: non_empty(std::env::var("USER_ID").ok()),
            access_token: non_empty(std::env::var("ACCESS_TOKEN").ok()),
            redirect_uri: non_empty(std::env::var("REDIRECT_URI").ok())
                .unwrap_or_else(|| DEFAULT_REDIRECT_URI.to_string()),
            scopes: non_empty(std::env::var("SCOPES").ok())
                .unwrap_or_else(|| DEFAULT_SCOPES.to_string()),
            env_path,
        })
    }

    /// Persist the access token under `ACCESS_TOKEN` in the `.env` file,
    /// preserving every other line.
    pub fn store_access_token(&self, token: &str) -> Result<()> {
        upsert_env_var(&self.env_path, ACCESS_TOKEN_KEY, token)
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

/// Set `key=value` in `path`, replacing an existing assignment or appending a
/// new one. Comments and unrelated lines are left untouched.
fn upsert_env_var(path: &Path, key: &str, value: &str) -> Result<()> {
    let existing = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };

    let prefix = format!("{key}=");
    let mut out = String::new();
    let mut replaced = false;

    for line in existing.lines() {
        let trimmed = line.trim_start();
        if !replaced && (trimmed.starts_with(&prefix) || trimmed == key) {
            out.push_str(&prefix);
            out.push_str(value);
            out.push('\n');
            replaced = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    if !replaced {
        out.push_str(&prefix);
        out.push_str(value);
        out.push('\n');
    }

    fs::write(path, out).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
