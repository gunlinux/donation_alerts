//! OAuth2 authorization-code flow: build the authorization URL and exchange
//! the returned code for an access token.

use anyhow::{bail, Context, Result};

use crate::config::{Config, AUTHORIZE_URL, TOKEN_URL};

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
    let access_token = payload
        .get("access_token")
        .and_then(|v| v.as_str())
        .context("token response has no access_token")?;

    config.store_access_token(access_token)?;

    println!("{}", serde_json::to_string_pretty(&payload)?);
    eprintln!("access token stored in .env as ACCESS_TOKEN");
    Ok(())
}
