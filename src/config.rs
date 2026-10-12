//! Configuration: environment loading and `.env` persistence.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const AUTHORIZE_URL: &str = "https://www.donationalerts.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://www.donationalerts.com/oauth/token";
pub const API_BASE: &str = "https://www.donationalerts.com/api/v1";
pub const WS_URL: &str = "wss://centrifugo.donationalerts.com/connection/websocket";

pub const DEFAULT_REDIRECT_URI: &str = "https://gunlinux.ru/callback/";
pub const DEFAULT_SCOPES: &str = "oauth-user-show oauth-donation-subscribe oauth-donation-index";

/// Secrets file holding the OAuth refresh material. Gitignored and written
/// with owner-only permissions; the refresh token is never put in `.env`.
pub const TOKEN_STORE_FILE: &str = ".tokens.json";

/// Renew the access token when it is within this many seconds of expiry.
pub const REFRESH_MARGIN_SECS: i64 = 60;

const ACCESS_TOKEN_KEY: &str = "ACCESS_TOKEN";

/// Tokens returned by the OAuth token endpoint, plus the access-token expiry
/// resolved to an absolute Unix timestamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

impl TokenSet {
    /// Parse a token-endpoint response, turning `expires_in` into an absolute
    /// `expires_at` relative to the current clock.
    pub fn from_response(payload: &Value) -> Result<Self> {
        let access_token = payload
            .get("access_token")
            .and_then(Value::as_str)
            .context("token response has no access_token")?
            .to_string();
        let refresh_token = payload
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::to_string);
        let expires_at = payload
            .get("expires_in")
            .and_then(Value::as_i64)
            .map(|secs| now_unix() + secs);
        Ok(Self {
            access_token,
            refresh_token,
            expires_at,
        })
    }
}

/// Current time as Unix seconds.
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Whether an access token with `expires_at` should be renewed at `now`.
/// An unknown expiry always forces a refresh attempt.
pub fn refresh_due(expires_at: Option<i64>, now: i64) -> bool {
    match expires_at {
        None => true,
        Some(expiry) => expiry - now <= REFRESH_MARGIN_SECS,
    }
}

/// Runtime configuration assembled from `.env` and the process environment.
pub struct Config {
    pub app_id: String,
    pub client_secret: Option<String>,
    pub user_id: Option<String>,
    pub access_token: Option<String>,
    pub rabbit_mq_dsn: Option<String>,
    pub redirect_uri: String,
    pub scopes: String,
    env_path: PathBuf,
    token_store_path: PathBuf,
}

impl Config {
    /// Load configuration from the default `.env` in the current directory.
    pub fn load() -> Result<Self> {
        Self::load_from(Path::new(".env"))
    }

    /// Load configuration, overlaying `env_path` on the process environment.
    ///
    /// A present-but-unparseable file is an error; a missing file is fine.
    /// Values defined in the file win over variables already present in the
    /// environment, so a token persisted by `token`/`refresh` is always the one
    /// used; keys the file omits still fall back to the environment.
    fn load_from(env_path: &Path) -> Result<Self> {
        if env_path.exists() {
            dotenvy::from_filename_override(env_path)
                .with_context(|| format!("parsing {}", env_path.display()))?;
        }

        let app_id = std::env::var("APP_ID").unwrap_or_default();
        if app_id.is_empty() {
            bail!("APP_ID is not set (put it in .env or the environment)");
        }

        Ok(Self {
            app_id,
            client_secret: non_empty(std::env::var("CLIENT_SECRET").ok()),
            user_id: non_empty(std::env::var("USER_ID").ok()),
            access_token: non_empty(std::env::var("ACCESS_TOKEN").ok()),
            rabbit_mq_dsn: non_empty(std::env::var("RABBIT_MQ_DSN").ok()),
            redirect_uri: non_empty(std::env::var("REDIRECT_URI").ok())
                .unwrap_or_else(|| DEFAULT_REDIRECT_URI.to_string()),
            scopes: non_empty(std::env::var("SCOPES").ok())
                .unwrap_or_else(|| DEFAULT_SCOPES.to_string()),
            env_path: env_path.to_path_buf(),
            token_store_path: PathBuf::from(TOKEN_STORE_FILE),
        })
    }

    /// Persist the access token under `ACCESS_TOKEN` in the `.env` file,
    /// preserving every other line.
    pub fn store_access_token(&self, token: &str) -> Result<()> {
        upsert_env_var(&self.env_path, ACCESS_TOKEN_KEY, token)
    }

    /// Read the stored refresh material, if any.
    pub fn stored_tokens(&self) -> Result<Option<TokenSet>> {
        read_tokens(&self.token_store_path)
    }

    /// Write the refresh material (and the current access token) to the
    /// secrets file.
    pub fn persist_tokens(&self, tokens: &TokenSet) -> Result<()> {
        write_tokens(&self.token_store_path, tokens)
    }

    /// Whether the listener should renew the access token before connecting:
    /// only when refresh material exists and the expiry is unknown or near.
    pub fn needs_refresh(&self) -> Result<bool> {
        let Some(tokens) = self.stored_tokens()? else {
            return Ok(false);
        };
        if tokens.refresh_token.is_none() {
            return Ok(false);
        }
        Ok(refresh_due(tokens.expires_at, now_unix()))
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

/// The RabbitMQ DSN, panicking when it is missing or empty.
///
/// Listening always publishes, so a missing broker address is a configuration
/// error that must stop the run before it connects to the gateway.
pub fn require_dsn(dsn: Option<String>) -> String {
    match dsn {
        Some(value) if !value.trim().is_empty() => value,
        _ => panic!("RABBIT_MQ_DSN is not set (put it in .env or the environment)"),
    }
}

/// Load a `TokenSet` from the secrets file; `None` when the file is absent.
fn read_tokens(path: &Path) -> Result<Option<TokenSet>> {
    match fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content)
            .with_context(|| format!("decoding {}", path.display()))
            .map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Write a `TokenSet` to the secrets file, readable only by the owner.
fn write_tokens(path: &Path, tokens: &TokenSet) -> Result<()> {
    let content = serde_json::to_string_pretty(tokens).context("encoding the token store")?;
    fs::write(path, content).with_context(|| format!("writing {}", path.display()))?;
    restrict_to_owner(path)?;
    Ok(())
}

/// Tighten `path` to owner-only (`0600`) on Unix; a no-op elsewhere.
#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("setting permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> Result<()> {
    Ok(())
}

/// Render a value for a `KEY=value` line, double-quoting it (and escaping `"` and `\`) when it
/// contains whitespace or a character that would otherwise change the line's meaning.
fn render_env_value(value: &str) -> String {
    let needs_quoting = value.is_empty()
        || value
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '#' | '"' | '\''));
    if !needs_quoting {
        return value.to_string();
    }

    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Set `key=value` in `path`, replacing an existing assignment or appending a
/// new one. Comments and unrelated lines are left untouched.
fn upsert_env_var(path: &Path, key: &str, value: &str) -> Result<()> {
    let rendered = render_env_value(value);
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
            out.push_str(&rendered);
            out.push('\n');
            replaced = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    if !replaced {
        out.push_str(&prefix);
        out.push_str(&rendered);
        out.push('\n');
    }

    fs::write(path, out).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    /// Serializes tests that read or mutate process environment variables.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("donationalerts-test-{nanos}-{name}"))
    }

    fn save_env(key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn restore_env(key: &str, prev: Option<String>) {
        match prev {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn parses_access_and_refresh_tokens() {
        let payload = json!({
            "access_token": "AAA",
            "refresh_token": "RRR",
            "expires_in": 3600,
        });
        let tokens = TokenSet::from_response(&payload).unwrap();
        assert_eq!(tokens.access_token, "AAA");
        assert_eq!(tokens.refresh_token.as_deref(), Some("RRR"));
        let expiry = tokens.expires_at.expect("expires_at should be set");
        assert!(
            expiry > now_unix(),
            "expiry {expiry} should be in the future"
        );
    }

    #[test]
    fn parses_without_refresh_token() {
        let tokens = TokenSet::from_response(&json!({ "access_token": "AAA" })).unwrap();
        assert_eq!(tokens.access_token, "AAA");
        assert_eq!(tokens.refresh_token, None);
        assert_eq!(tokens.expires_at, None);
    }

    #[test]
    fn parses_without_expiry() {
        let payload = json!({ "access_token": "AAA", "refresh_token": "RRR" });
        let tokens = TokenSet::from_response(&payload).unwrap();
        assert_eq!(tokens.expires_at, None);
    }

    #[test]
    fn missing_access_token_is_an_error() {
        let err = TokenSet::from_response(&json!({ "refresh_token": "RRR" })).unwrap_err();
        assert!(err.to_string().contains("access_token"));
    }

    #[test]
    fn refresh_due_predicate() {
        let now = 1_000_000;
        assert!(refresh_due(None, now), "unknown expiry forces a refresh");
        assert!(refresh_due(Some(now - 1), now), "past expiry");
        assert!(
            refresh_due(Some(now + REFRESH_MARGIN_SECS), now),
            "within margin"
        );
        assert!(
            !refresh_due(Some(now + REFRESH_MARGIN_SECS + 1), now),
            "comfortably in the future"
        );
    }

    #[test]
    fn token_store_round_trips() {
        let path = temp_path("tokens.json");
        let tokens = TokenSet {
            access_token: "AAA".into(),
            refresh_token: Some("RRR".into()),
            expires_at: Some(1_234_567),
        };
        write_tokens(&path, &tokens).unwrap();
        assert_eq!(read_tokens(&path).unwrap().as_ref(), Some(&tokens));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn missing_store_reads_as_none() {
        let path = temp_path("absent.json");
        assert_eq!(read_tokens(&path).unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn token_store_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_path("perms.json");
        write_tokens(
            &path,
            &TokenSet {
                access_token: "AAA".into(),
                refresh_token: Some("RRR".into()),
                expires_at: None,
            },
        )
        .unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "token store must be 0600, got {mode:o}");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn refresh_token_never_lands_in_env() {
        let env = temp_path(".env");
        let store = temp_path("store.json");
        let tokens = TokenSet {
            access_token: "AAA".into(),
            refresh_token: Some("SUPER-SECRET".into()),
            expires_at: Some(42),
        };
        upsert_env_var(&env, ACCESS_TOKEN_KEY, &tokens.access_token).unwrap();
        write_tokens(&store, &tokens).unwrap();

        let env_content = fs::read_to_string(&env).unwrap();
        assert!(env_content.contains("AAA"));
        assert!(
            !env_content.contains("SUPER-SECRET"),
            "refresh token leaked into .env"
        );

        let _ = fs::remove_file(&env);
        let _ = fs::remove_file(&store);
    }

    #[test]
    fn env_only_key_falls_back_to_environment() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev_app = save_env("APP_ID");
        let prev_redirect = save_env("REDIRECT_URI");
        std::env::set_var("APP_ID", "env-app");
        std::env::set_var("REDIRECT_URI", "https://fallback.example/");

        let env_file = temp_path(".env-fallback");
        fs::write(&env_file, "# nothing the CLI manages here\n").unwrap();

        let config = Config::load_from(&env_file).unwrap();
        assert_eq!(
            config.app_id, "env-app",
            "environment APP_ID should be used"
        );
        assert_eq!(
            config.redirect_uri, "https://fallback.example/",
            "environment REDIRECT_URI should be used when .env omits it"
        );

        let _ = fs::remove_file(&env_file);
        restore_env("APP_ID", prev_app);
        restore_env("REDIRECT_URI", prev_redirect);
    }

    #[test]
    fn dotenv_overrides_exported_variables() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev_app = save_env("APP_ID");
        let prev_token = save_env("ACCESS_TOKEN");
        std::env::set_var("APP_ID", "env-app");
        std::env::set_var("ACCESS_TOKEN", "old-exported-token");

        let env_file = temp_path(".env-override");
        fs::write(
            &env_file,
            "APP_ID=file-app\nACCESS_TOKEN=new-stored-token\n",
        )
        .unwrap();

        let config = Config::load_from(&env_file).unwrap();
        assert_eq!(config.app_id, "file-app", ".env APP_ID should win");
        assert_eq!(
            config.access_token.as_deref(),
            Some("new-stored-token"),
            ".env ACCESS_TOKEN should win over the exported value"
        );

        let _ = fs::remove_file(&env_file);
        restore_env("APP_ID", prev_app);
        restore_env("ACCESS_TOKEN", prev_token);
    }

    #[test]
    fn rabbit_dsn_follows_env_precedence() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev_app = save_env("APP_ID");
        let prev_dsn = save_env("RABBIT_MQ_DSN");
        std::env::set_var("APP_ID", "x");
        std::env::set_var("RABBIT_MQ_DSN", "amqp://env-host");

        let env_file = temp_path(".env-rabbit");
        fs::write(&env_file, "RABBIT_MQ_DSN=amqp://file-host\n").unwrap();

        let config = Config::load_from(&env_file).unwrap();
        assert_eq!(
            config.rabbit_mq_dsn.as_deref(),
            Some("amqp://file-host"),
            ".env RABBIT_MQ_DSN should win over the exported value"
        );

        let _ = fs::remove_file(&env_file);
        restore_env("APP_ID", prev_app);
        restore_env("RABBIT_MQ_DSN", prev_dsn);
    }

    #[test]
    fn rabbit_dsn_unset_is_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev_app = save_env("APP_ID");
        let prev_dsn = save_env("RABBIT_MQ_DSN");
        std::env::set_var("APP_ID", "x");
        std::env::remove_var("RABBIT_MQ_DSN");

        let env_file = temp_path(".env-no-rabbit");
        fs::write(&env_file, "# nothing\n").unwrap();

        let config = Config::load_from(&env_file).unwrap();
        assert_eq!(config.rabbit_mq_dsn, None);

        let _ = fs::remove_file(&env_file);
        restore_env("APP_ID", prev_app);
        restore_env("RABBIT_MQ_DSN", prev_dsn);
    }

    #[test]
    fn env_example_parses() {
        // Parsed without mutating the process environment, so this only checks
        // that the shipped example is well-formed.
        let pairs: Vec<_> = dotenvy::from_path_iter(Path::new(".env.example"))
            .expect(".env.example must parse")
            .map(|entry| entry.expect("each line must parse"))
            .collect();
        assert!(
            pairs.iter().any(|(k, _)| k == "APP_ID"),
            "the example should still define the OAuth keys"
        );
        assert!(
            !pairs.iter().any(|(k, _)| k == "RABBIT_MQ_DSN"),
            "RABBIT_MQ_DSN is commented out in the example"
        );
    }

    #[test]
    fn require_dsn_passes_a_present_value_through() {
        assert_eq!(require_dsn(Some("amqp://host".to_string())), "amqp://host");
    }

    #[test]
    #[should_panic(expected = "RABBIT_MQ_DSN")]
    fn require_dsn_panics_when_unset() {
        require_dsn(None);
    }

    #[test]
    #[should_panic(expected = "RABBIT_MQ_DSN")]
    fn require_dsn_panics_when_empty() {
        require_dsn(Some("   ".to_string()));
    }

    #[test]
    fn malformed_env_is_an_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        let env_file = temp_path(".env-malformed");
        fs::write(&env_file, "TEST_KEY=1\nSCOPES=a b c\n").unwrap();

        let err = match Config::load_from(&env_file) {
            Ok(_) => panic!("a malformed .env should not load"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains(".env"),
            "error should name the file: {err}"
        );

        let _ = fs::remove_file(&env_file);
        std::env::remove_var("TEST_KEY");
    }

    #[test]
    fn missing_env_falls_back_to_environment() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev_app = save_env("APP_ID");
        std::env::set_var("APP_ID", "env-only-app");

        let missing = temp_path(".env-does-not-exist");
        let config = Config::load_from(&missing).unwrap();
        assert_eq!(config.app_id, "env-only-app");

        restore_env("APP_ID", prev_app);
    }

    #[test]
    fn spaced_value_round_trips_through_writer() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev_app = save_env("APP_ID");
        let prev_scopes = save_env("SCOPES");
        std::env::set_var("APP_ID", "x");

        let env_file = temp_path(".env-writer");
        upsert_env_var(
            &env_file,
            "SCOPES",
            "oauth-user-show oauth-donation-subscribe",
        )
        .unwrap();
        let config = Config::load_from(&env_file).unwrap();
        assert_eq!(config.scopes, "oauth-user-show oauth-donation-subscribe");
        assert!(fs::read_to_string(&env_file)
            .unwrap()
            .contains("SCOPES=\"oauth-user-show oauth-donation-subscribe\""));

        let _ = fs::remove_file(&env_file);
        restore_env("APP_ID", prev_app);
        restore_env("SCOPES", prev_scopes);
    }
}
