//! OAuth2 authorization-code flow: build the authorization URL and exchange
//! the returned code for an access token.

use anyhow::{bail, Context, Result};

use crate::config::{Config, TokenSet, AUTHORIZE_URL, TOKEN_URL};

/// Build the URL the user opens to grant access and receive a code.
pub fn authorize_url(config: &Config) -> Result<String> {
    let url = reqwest::Url::parse_with_params(
        AUTHORIZE_URL,
        &[
            ("response_type", "code"),
            ("client_id", config.app_id.as_str()),
            ("redirect_uri", config.redirect_uri.as_str()),
            ("scope", config.scopes.as_str()),
        ],
    )
    .context("building the authorization URL")?;
    Ok(url.into())
}

/// Exchange an authorization code for an access token, storing it in `.env`.
pub async fn exchange_and_store(config: &Config, code: &str) -> Result<()> {
    let client_secret = config
        .client_secret
        .as_deref()
        .context("CLIENT_SECRET is not set (put it in .env or the environment)")?;

    let response = reqwest::Client::new()
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", config.app_id.as_str()),
            ("client_secret", client_secret),
            ("redirect_uri", config.redirect_uri.as_str()),
            ("code", code),
        ])
        .send()
        .await
        .context("calling the token endpoint")?;

    let status = response.status();
    let body = response
        .text()
        .await
        .context("reading the token response")?;
    if !status.is_success() {
        bail!("token request failed: HTTP {status}: {body}");
    }

    let payload: serde_json::Value =
        serde_json::from_str(&body).context("decoding the token response")?;
    let tokens = TokenSet::from_response(&payload)?;

    config.store_access_token(&tokens.access_token)?;
    config.persist_tokens(&tokens)?;

    println!("{}", serde_json::to_string_pretty(&payload)?);
    eprintln!("access token stored in .env as ACCESS_TOKEN");
    Ok(())
}

/// Exchange the stored refresh token for a fresh access token, updating both
/// `.env` (access token) and the secrets file (refresh material + expiry).
pub async fn refresh_and_store(config: &mut Config) -> Result<()> {
    let client_secret = config
        .client_secret
        .as_deref()
        .context("CLIENT_SECRET is not set (put it in .env or the environment)")?;

    let stored = config.stored_tokens()?;
    let refresh_token = stored
        .as_ref()
        .and_then(|t| t.refresh_token.clone())
        .context("no stored refresh token — run `donationalerts token <CODE>` first")?;

    let response = reqwest::Client::new()
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", config.app_id.as_str()),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token.as_str()),
        ])
        .send()
        .await
        .context("calling the token endpoint")?;

    let status = response.status();
    let body = response
        .text()
        .await
        .context("reading the refresh response")?;
    if !status.is_success() {
        bail!("refresh request failed: HTTP {status}: {body}");
    }

    let payload: serde_json::Value =
        serde_json::from_str(&body).context("decoding the refresh response")?;
    let mut tokens = TokenSet::from_response(&payload)?;
    // Endpoints that do not rotate the refresh token omit it; retain the old one.
    if tokens.refresh_token.is_none() {
        tokens.refresh_token = Some(refresh_token);
    }

    config.store_access_token(&tokens.access_token)?;
    config.persist_tokens(&tokens)?;
    config.access_token = Some(tokens.access_token.clone());

    println!("{}", serde_json::to_string_pretty(&payload)?);
    eprintln!("access token refreshed");
    Ok(())
}
