//! Live donation stream: Centrifugo WebSocket handshake, channel
//! subscription, and printing of incoming donation pushes.

use std::io::Write;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use crate::auth;
use crate::config::{self, Config, API_BASE, WS_URL};
use crate::publish::{self, Publisher};

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
    // Listening always publishes, so a missing broker address panics here,
    // before any gateway connection is attempted.
    let dsn = config::require_dsn(config.rabbit_mq_dsn.clone());
    let mut publisher = publish::RabbitPublisher::new(dsn);
    if let Err(e) = publisher.preflight().await {
        eprintln!("could not connect to RabbitMQ: {e:#}; will retry on each donation");
    }

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

        match run_once(&access_token, &channel, &mut publisher).await {
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

async fn run_once<P: Publisher>(
    access_token: &str,
    channel: &str,
    publisher: &mut P,
) -> Result<()> {
    let http = reqwest::Client::new();

    let connection_token = fetch_connection_token(&http, access_token).await?;

    let (mut ws, _) = tokio_tungstenite::connect_async(WS_URL)
        .await
        .context("connecting to the Centrifugo websocket")?;
    let client_uid = handshake(&mut ws, &connection_token).await?;

    let subscription_token = create_subscription(&http, access_token, channel, &client_uid).await?;
    subscribe(&mut ws, channel, &subscription_token).await?;

    eprintln!("subscribed to {channel}; waiting for donations");
    drain(&mut ws, publisher).await
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

/// Read frames until the connection closes, printing and publishing each donation.
async fn drain<P: Publisher>(ws: &mut Ws, publisher: &mut P) -> Result<()> {
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
                    Some(Ok(Message::Text(text))) => {
                        handle_frame(text.as_str(), publisher, &mut std::io::stdout()).await;
                    }
                    Some(Ok(Message::Close(_))) => return Ok(()),
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

/// Why a gateway frame did not yield a usable donation.
#[derive(Debug, PartialEq)]
enum PayloadError {
    /// A publication frame carried no recognizable donation payload.
    Missing,
    /// A payload carried as a string that is not valid JSON.
    Undecodable(String),
}

/// Locate the donation carried by a gateway frame.
///
/// DonationAlerts wraps every server push in a top-level `result` object
/// (Centrifugo 2.x), so a donation lives at `result.data.data`, as in
/// `{"result":{"type":1,"channel":"...","data":{"seq":N,"data":{...}}}}`.
/// The raw Centrifugo shapes (`push.pub.data`, then `push.data`) are accepted
/// as a fallback. `Ok(None)` means the frame is not a donation publication (a
/// command reply, the subscription notice, or a keepalive) and `Err(_)` means a
/// publication was received whose payload could not be read.
fn extract_donation(frame: &Value) -> Result<Option<Value>, PayloadError> {
    // Command replies (and only replies) carry an `id`; everything else is a
    // server push.
    if frame.get("id").is_some() {
        return Ok(None);
    }
    if let Some(result) = frame.get("result") {
        return extract_from_result(result);
    }
    if let Some(push) = frame.get("push") {
        return extract_from_push(push);
    }
    Ok(None)
}

/// Read the donation from a DonationAlerts `result`-wrapped push.
///
/// The publication payload is nested one level deep at `result.data.data`.
/// A `result.data` object without a `data` member is the subscription notice
/// (it carries `info`) or another non-donation push, and yields `Ok(None)`.
fn extract_from_result(result: &Value) -> Result<Option<Value>, PayloadError> {
    let Some(publication) = result.get("data").and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(payload) = publication.get("data") else {
        return Ok(None);
    };
    match payload {
        Value::Object(_) => Ok(Some(payload.clone())),
        Value::String(encoded) => decode_payload_string(encoded).map(Some),
        _ => Err(PayloadError::Missing),
    }
}

/// Read the donation from a raw Centrifugo `push` frame.
fn extract_from_push(push: &Value) -> Result<Option<Value>, PayloadError> {
    let payload = push
        .get("pub")
        .and_then(|publication| publication.get("data"))
        .or_else(|| push.get("data"))
        .ok_or(PayloadError::Missing)?;
    match payload {
        Value::String(encoded) => decode_payload_string(encoded).map(Some),
        value => Ok(Some(value.clone())),
    }
}

/// Decode a JSON-encoded payload string into a value.
fn decode_payload_string(encoded: &str) -> Result<Value, PayloadError> {
    serde_json::from_str(encoded).map_err(|e| PayloadError::Undecodable(e.to_string()))
}

/// Print and publish the donation carried by a push frame; ignore everything else.
///
/// `out` receives the donation JSON line (standard output in the listener);
/// the same bytes are published to RabbitMQ.
async fn handle_frame<P: Publisher, W: Write>(raw: &str, publisher: &mut P, out: &mut W) {
    let frame: Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(e) => {
            eprintln!("could not parse websocket frame: {e}");
            return;
        }
    };

    match extract_donation(&frame) {
        Ok(Some(donation)) => match publish::message_body(&donation) {
            Ok(body) => {
                if let Err(e) = out.write_all(&body).and_then(|_| out.write_all(b"\n")) {
                    eprintln!("could not write donation to stdout: {e}");
                }
                publish::publish_with_retry(publisher, &body).await;
            }
            Err(e) => eprintln!("could not serialize donation: {e}"),
        },
        Ok(None) => {} // command reply, subscription notice, or keepalive
        Err(PayloadError::Missing) => {
            eprintln!(
                "unrecognized push frame (no donation under result.data.data, \
                 push.pub.data, or push.data): {raw}"
            );
        }
        Err(PayloadError::Undecodable(e)) => {
            eprintln!("could not parse donation payload: {e}");
        }
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

    #[test]
    fn donation_under_the_result_envelope_is_extracted() {
        // Captured from the live DonationAlerts gateway (Centrifugo 2.5.1):
        // pushes are wrapped in a top-level `result`, donation at `result.data.data`.
        let frame = json!({
            "result": {
                "type": 1,
                "channel": "$alerts:donation_5116810",
                "data": {
                    "seq": 227,
                    "data": {
                        "id": 189804356,
                        "name": "Donations",
                        "username": "test",
                        "message": "test",
                        "amount": 555,
                        "currency": "RUB",
                        "recipient_name": "gunlinux",
                        "recipient": { "user_id": 5116810 },
                        "created_at": "2026-08-26 07:33:34"
                    }
                }
            }
        });
        let donation = extract_donation(&frame).unwrap().unwrap();
        assert_eq!(donation["id"], 189804356);
        assert_eq!(donation["amount"], 555);
        assert_eq!(donation["recipient"]["user_id"], 5116810);
    }

    #[test]
    fn subscription_notice_under_the_result_envelope_is_ignored() {
        // The gateway pushes this on the channel right after subscribing; it
        // must be ignored without a diagnostic.
        let frame = json!({
            "result": {
                "type": 1,
                "channel": "$alerts:donation_5116810",
                "data": { "info": { "user": "Client:21819", "client": "abc-123" } }
            }
        });
        assert_eq!(extract_donation(&frame), Ok(None));
    }

    #[test]
    fn result_wrapped_payload_delivered_as_an_encoded_string_is_decoded() {
        let frame = json!({
            "result": {
                "channel": "$alerts:donation_1",
                "data": { "seq": 1, "data": "{\"id\":7,\"amount\":9}" }
            }
        });
        let donation = extract_donation(&frame).unwrap().unwrap();
        assert_eq!(donation["id"], 7);
        assert_eq!(donation["amount"], 9);
    }

    #[test]
    fn result_wrapped_publication_with_an_unusable_payload_is_reported() {
        let frame = json!({
            "result": {
                "channel": "$alerts:donation_1",
                "data": { "seq": 1, "data": 5 }
            }
        });
        assert_eq!(extract_donation(&frame), Err(PayloadError::Missing));
    }

    #[test]
    fn command_reply_with_an_id_is_ignored() {
        let frame = json!({
            "id": 2,
            "result": { "recoverable": true, "seq": 12, "epoch": "1787823005" }
        });
        assert_eq!(extract_donation(&frame), Ok(None));
    }

    #[test]
    fn donation_nested_under_the_pub_envelope_is_extracted() {
        let frame = json!({
            "push": {
                "channel": "$alerts:donation_1",
                "pub": { "data": { "id": 1, "username": "alice", "amount": 100 } }
            }
        });
        let donation = extract_donation(&frame).unwrap().unwrap();
        assert_eq!(donation["username"], "alice");
        assert_eq!(donation["amount"], 100);
    }

    #[test]
    fn donation_carried_directly_on_the_push_is_extracted() {
        let frame = json!({
            "push": { "channel": "$alerts:donation_1", "data": { "id": 2 } }
        });
        let donation = extract_donation(&frame).unwrap().unwrap();
        assert_eq!(donation["id"], 2);
    }

    #[test]
    fn string_encoded_payload_is_decoded() {
        let frame = json!({
            "push": { "channel": "$alerts:donation_1", "pub": { "data": "{\"id\":3,\"amount\":50}" } }
        });
        let donation = extract_donation(&frame).unwrap().unwrap();
        assert_eq!(donation["amount"], 50);
    }

    #[test]
    fn push_without_a_payload_is_reported_not_silently_dropped() {
        let frame = json!({
            "push": { "channel": "$alerts:donation_1", "info": {} }
        });
        assert_eq!(extract_donation(&frame), Err(PayloadError::Missing));
    }

    #[test]
    fn non_push_frame_is_ignored_silently() {
        let frame = json!({ "id": 2, "result": { "channel": "$alerts:donation_1" } });
        assert_eq!(extract_donation(&frame), Ok(None));
    }

    #[test]
    fn undecodable_payload_is_reported() {
        let frame = json!({ "push": { "pub": { "data": "not json" } } });
        assert!(matches!(
            extract_donation(&frame),
            Err(PayloadError::Undecodable(_))
        ));
    }

    /// Records every published body so the listener can be exercised without a
    /// live broker.
    #[derive(Default)]
    struct RecordingPublisher {
        calls: Vec<Vec<u8>>,
    }

    impl Publisher for RecordingPublisher {
        async fn publish(&mut self, body: &[u8]) -> Result<()> {
            self.calls.push(body.to_vec());
            Ok(())
        }
    }

    struct FailingPublisher;

    impl Publisher for FailingPublisher {
        async fn publish(&mut self, _body: &[u8]) -> Result<()> {
            Err(anyhow::anyhow!("broker down"))
        }
    }

    #[tokio::test]
    async fn donation_frame_is_published_and_printed() {
        let raw = r#"{"result":{"type":1,"channel":"$alerts:donation_5116810",
            "data":{"seq":227,"data":{"id":7,"name":null,"message":"","amount":9}}}}"#;
        let mut publisher = RecordingPublisher::default();
        let mut out: Vec<u8> = Vec::new();
        handle_frame(raw, &mut publisher, &mut out).await;

        assert_eq!(publisher.calls.len(), 1, "exactly one publish");
        let body: Value = serde_json::from_slice(&publisher.calls[0]).unwrap();
        assert_eq!(body["id"], 7);
        assert!(body["name"].is_null());
        assert_eq!(body["message"], "");

        let printed = String::from_utf8(out).unwrap();
        assert!(
            printed.ends_with('\n'),
            "stdout line must end with a newline"
        );
        let printed_value: Value = serde_json::from_str(printed.trim_end()).unwrap();
        assert_eq!(
            printed_value, body,
            "stdout must carry the same donation as the publish"
        );
    }

    #[tokio::test]
    async fn non_donation_frames_publish_nothing() {
        let frames = [
            // command reply
            r#"{"id":2,"result":{"recoverable":true,"seq":12}}"#,
            // subscription notice
            r#"{"result":{"type":1,"channel":"$alerts:donation_1","data":{"info":{"user":"Client:1"}}}}"#,
            // undecodable publication payload
            r#"{"push":{"channel":"$alerts:donation_1","pub":{"data":"not json"}}}"#,
        ];
        let mut publisher = RecordingPublisher::default();
        let mut out: Vec<u8> = Vec::new();
        for raw in frames {
            handle_frame(raw, &mut publisher, &mut out).await;
        }
        assert_eq!(
            publisher.calls.len(),
            0,
            "no non-donation frame may publish"
        );
        assert!(out.is_empty(), "no non-donation frame may print");
    }

    #[tokio::test]
    async fn a_publish_failure_does_not_stop_processing() {
        let mut publisher = FailingPublisher;
        let mut out: Vec<u8> = Vec::new();
        let first = r#"{"result":{"channel":"c","data":{"seq":1,"data":{"id":1}}}}"#;
        let second = r#"{"result":{"channel":"c","data":{"seq":2,"data":{"id":2}}}}"#;
        handle_frame(first, &mut publisher, &mut out).await;
        handle_frame(second, &mut publisher, &mut out).await;
        assert_eq!(
            out.iter().filter(|b| **b == b'\n').count(),
            2,
            "both donations were still printed after a publish failure"
        );
    }
}
