//! Live donation stream: Centrifugo WebSocket handshake, channel
//! subscription, and printing of incoming donation pushes.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use crate::auth;
use crate::config::{Config, API_BASE, WS_URL};

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const RECONNECT_DELAY: Duration = Duration::from_secs(5);
const PING_INTERVAL: Duration = Duration::from_secs(30);

/// An access token was rejected (HTTP 401). Distinct from transport errors so
/// the listener can refresh once and then give up instead of looping.
#[derive(Debug)]
struct AuthFailure(String);

impl std::fmt::Display for AuthFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for AuthFailure {}

/// Connect and print donations, reconnecting on transport failures. Renews the
/// access token before connecting when it is due, and once more if the gateway
/// rejects it, exiting with guidance when renewal is impossible.
pub async fn listen(config: &mut Config) -> Result<()> {
    let user_id = config
        .user_id
        .as_deref()
        .context("USER_ID is not set (put it in .env or the environment)")?
        .to_string();
    let channel = format!("$alerts:donation_{user_id}");

    if config.needs_refresh()? {
        match auth::refresh_and_store(config).await {
            Ok(()) => eprintln!("access token refreshed before connecting"),
            Err(e) => {
                eprintln!("could not refresh access token: {e:#}; using the stored token")
            }
        }
    }

    let mut refreshed_since_connect = false;
    loop {
        let access_token = config
            .access_token
            .clone()
            .context("ACCESS_TOKEN is not set in .env — run `donationalerts token <CODE>` first")?;

        match run_once(&access_token, &channel).await {
            Ok(()) => {
                refreshed_since_connect = false;
                eprintln!("connection closed by the gateway; reconnecting");
            }
            Err(e) if e.downcast_ref::<AuthFailure>().is_some() => {
                if refreshed_since_connect {
                    return Err(anyhow!(
                        "access token is still rejected after refreshing: {e:#}. \
                         Re-run `donationalerts token <CODE>`"
                    ));
                }
                eprintln!("access token rejected; attempting to refresh: {e:#}");
                auth::refresh_and_store(config)
                    .await
                    .context("refreshing the rejected access token")?;
                refreshed_since_connect = true;
                continue;
            }
            Err(e) => {
                refreshed_since_connect = false;
                eprintln!("stream error: {e:#}; reconnecting");
            }
        }
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

async fn run_once(access_token: &str, channel: &str) -> Result<()> {
    let http = reqwest::Client::new();

    let connection_token = fetch_connection_token(&http, access_token).await?;

    let (mut ws, _) = tokio_tungstenite::connect_async(WS_URL)
        .await
        .context("connecting to the Centrifugo websocket")?;
    let client_uid = handshake(&mut ws, &connection_token).await?;

    let subscription_token = create_subscription(&http, access_token, channel, &client_uid).await?;
    subscribe(&mut ws, channel, &subscription_token).await?;

    eprintln!("subscribed to {channel}; waiting for donations");
    drain(&mut ws).await
}

/// GET the socket connection token for the authorized user.
async fn fetch_connection_token(http: &reqwest::Client, access_token: &str) -> Result<String> {
    let response = http
        .get(format!("{API_BASE}/user/oauth"))
        .bearer_auth(access_token)
        .send()
        .await
        .context("requesting the connection token")?;

    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .context("decoding the connection token response")?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(anyhow::Error::new(AuthFailure(format!(
            "user/oauth rejected the access token (HTTP 401): {payload}"
        ))));
    }
    if !status.is_success() {
        bail!("user/oauth failed: HTTP {status}: {payload}");
    }

    payload
        .pointer("/data/socket_connection_token")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .context("response has no socket_connection_token")
}

/// Send the connect frame and return the Centrifugo client uid.
async fn handshake(ws: &mut Ws, connection_token: &str) -> Result<String> {
    let frame = json!({ "params": { "token": connection_token }, "id": 1 });
    ws.send(Message::Text(frame.to_string().into()))
        .await
        .context("sending the connect frame")?;

    let reply = read_frame(ws)
        .await?
        .context("websocket closed during handshake")?;
    check_error(&reply)?;
    reply
        .pointer("/result/client")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .context("connect reply has no client uid")
}

/// Request a subscription token for the channel from the REST API.
async fn create_subscription(
    http: &reqwest::Client,
    access_token: &str,
    channel: &str,
    client_uid: &str,
) -> Result<String> {
    let response = http
        .post(format!("{API_BASE}/centrifuge/subscribe"))
        .bearer_auth(access_token)
        .json(&json!({ "channels": [channel], "client": client_uid }))
        .send()
        .await
        .context("requesting the subscription token")?;

    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .context("decoding the subscription token response")?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(anyhow::Error::new(AuthFailure(format!(
            "centrifuge/subscribe rejected the access token (HTTP 401): {payload}"
        ))));
    }
    if !status.is_success() {
        bail!("centrifuge/subscribe failed: HTTP {status}: {payload}");
    }

    payload
        .pointer("/channels/0/token")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .context("subscribe response has no channel token")
}

/// Send the subscribe frame and confirm the gateway accepted it.
async fn subscribe(ws: &mut Ws, channel: &str, token: &str) -> Result<()> {
    let frame = json!({ "params": { "channel": channel, "token": token }, "method": 1, "id": 2 });
    ws.send(Message::Text(frame.to_string().into()))
        .await
        .context("sending the subscribe frame")?;

    let reply = read_frame(ws)
        .await?
        .context("websocket closed during subscribe")?;
    check_error(&reply)
}

/// Read frames until the connection closes, printing each donation.
async fn drain(ws: &mut Ws) -> Result<()> {
    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            _ = ping.tick() => {
                ws.send(Message::Ping(Vec::new().into())).await.context("sending ping")?;
            }
            message = ws.next() => {
                match message {
                    None => return Ok(()),
                    Some(Err(e)) => return Err(e).context("websocket receive failed"),
                    Some(Ok(Message::Text(text))) => handle_text(text.as_str()),
                    Some(Ok(Message::Close(_))) => return Ok(()),
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

/// Print the donation carried by a push frame; ignore everything else.
fn handle_text(raw: &str) {
    let frame: Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(e) => {
            eprintln!("could not parse websocket frame: {e}");
            return;
        }
    };

    let Some(push) = frame.get("push") else {
        return; // command reply or keepalive
    };
    let data = match push.get("data") {
        Some(Value::String(s)) => match serde_json::from_str::<Value>(s) {
            Ok(value) => value,
            Err(e) => {
                eprintln!("could not parse donation payload: {e}");
                return;
            }
        },
        Some(value) => value.clone(),
        None => return,
    };

    match serde_json::to_string(&data) {
        Ok(line) => println!("{line}"),
        Err(e) => eprintln!("could not serialize donation: {e}"),
    }
}

/// Receive the next text frame, skipping non-text frames.
async fn read_frame(ws: &mut Ws) -> Result<Option<Value>> {
    while let Some(message) = ws.next().await {
        match message.context("websocket receive failed")? {
            Message::Text(text) => {
                return serde_json::from_str(text.as_str())
                    .map(Some)
                    .context("decoding a gateway frame");
            }
            Message::Close(_) => return Ok(None),
            _ => {}
        }
    }
    Ok(None)
}

/// Fail if a gateway frame reports an error.
fn check_error(frame: &Value) -> Result<()> {
    match frame.get("error") {
        Some(error) => Err(anyhow!("gateway error: {error}")),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_failure_is_distinguishable_from_other_errors() {
        let auth = anyhow::Error::new(AuthFailure("401".into()));
        assert!(auth.downcast_ref::<AuthFailure>().is_some());

        let transport = anyhow!("connection reset");
        assert!(transport.downcast_ref::<AuthFailure>().is_none());
    }
}
