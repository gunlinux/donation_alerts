## 1. Configuration and dependency

- [x] 1.1 Add the AMQP client dependency (`lapin`) to `Cargo.toml` with default features off and a rustls TLS feature; verify `cargo build` succeeds and `cargo tree -i openssl-sys` / `cargo tree -i native-tls` report no such crate in the tree
- [x] 1.2 Add a `rabbit_mq_dsn: Option<String>` field to `Config`, read from `RABBIT_MQ_DSN` through the existing `.env`-over-environment path; verify a config unit test proves `.env` wins over an exported `RABBIT_MQ_DSN` and that an unset key stays `None`
- [x] 1.3 Add a `require_dsn(Option<String>) -> String` (or equivalent) helper that panics with a message naming `RABBIT_MQ_DSN` for a missing or empty value; verify `#[should_panic]` tests cover the unset case and the empty-string case, and a test covers a present value passing through unchanged
- [x] 1.4 Document `RABBIT_MQ_DSN` in `.env.example` (with a commented example value) and note it is required by `listen`; verify the example file parses with the existing loader (a `Config::load_from` test on the example text, or manual load)

## 2. RabbitMQ publisher

- [x] 2.1 Add `src/publish.rs` with a `RabbitPublisher` that connects to the DSN, declares a durable queue named `donations`, and publishes each body to the default exchange with routing key `donations`; verify the queue name, routing key, and `delivery_mode = 2` are covered by unit tests over the extracted constants/properties builder
- [x] 2.2 Build the message body as `serde_json::to_vec(&donation)` for the raw `Value`; verify a unit test round-trips a donation with null `name`/`message` and an unknown field and asserts both survive unchanged (content, not key order)
- [x] 2.3 Implement lazy reconnect: on a publish failure, report on stderr, drop the dead connection/channel, reconnect with the listener's backoff, re-declare the queue, and retry the donation once; verify via a `Publisher` seam (trait) and a recording fake — a failing publisher is retried, a permanently failing broker surfaces a stderr diagnostic rather than panicking the loop
- [x] 2.4 Make the publisher testable behind a small `Publisher` trait (async `publish(&mut self, body: &[u8])`) implemented by `RabbitPublisher`; verify the listener compiles against the trait and unit tests can inject a fake without a live broker

## 3. Listener wiring

- [x] 3.1 In `src/stream.rs`, build the publisher once in `listen` after resolving `RABBIT_MQ_DSN` with `require_dsn`, so a missing/empty DSN panics before any gateway connection; verify a test asserts the panic occurs before connection setup (DSN resolution is a separate, testable step)
- [x] 3.2 Pass the publisher through `run_once`/`drain`/`handle_text` and publish each extracted donation in addition to printing it; verify a unit test with a recording fake asserts a donation frame produces exactly one publish whose body is the donation JSON, and that stdout printing still happens
- [x] 3.3 Ensure non-donation frames (command replies, subscription notices, keepalives, undecodable pushes) publish nothing; verify a unit test asserts the fake records zero publishes for those frames
- [x] 3.4 Keep the listener resilient: a publish/reconnect failure must not tear down the websocket stream or panic; verify a unit test feeds a donation while the fake publisher returns an error and asserts the loop continues to the next frame

## 4. Documentation

- [x] 4.1 Update `README.md` to document that `listen` publishes every donation to the durable `donations` queue (default exchange, persistent) as well as printing it, and that `RABBIT_MQ_DSN` is required or `listen` panics

## 5. Verification

- [x] 5.1 Run the standing checks and confirm all are clean: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`
- [x] 5.2 Against a reachable RabbitMQ (local or the workspace broker), run `listen` with `RABBIT_MQ_DSN` set, drive one donation, and confirm the message is on the `donations` queue (for example `rabbitmqadmin get queue=donations`) with the raw donation JSON and persistent delivery; if no broker is reachable in this environment, record that the live half could not be observed and state which unit tests cover the behavior instead
  - Live broker half NOT observed: no Docker daemon running, nothing listening on `localhost:5672`, no `rabbitmqadmin`/`rabbitmqctl`, and `.env` has no `RABBIT_MQ_DSN`. Running `cargo run -- listen` without a DSN panics as specified — `panicked at src/config.rs: RABBIT_MQ_DSN is not set (put it in .env or the environment)`, exit 101 — confirming the fail-fast path end to end before any network I/O. Queue/routing/delivery-mode are covered by `publish::tests::queue_and_routing_key_match` and `publish::tests::published_messages_are_persistent`; the publish-on-donation path by `stream::tests::donation_frame_is_published_and_printed`; and reply/notice/undecodable frames publish nothing per `stream::tests::non_donation_frames_publish_nothing`.
