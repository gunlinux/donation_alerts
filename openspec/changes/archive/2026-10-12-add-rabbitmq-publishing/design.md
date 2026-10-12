## Context

See proposal.md — Why. The listener (`src/stream.rs`) already extracts each donation as a
`serde_json::Value` and prints it as one JSON line from `handle_text`. Configuration
(`src/config.rs`) loads managed keys from `.env` (overriding the environment) and exposes them
on `Config`. The project's TLS stack is rustls-only (`reqwest` with `rustls-tls`,
`tokio-tungstenite` with `rustls-tls-webpki-roots`), and the sibling services in this workspace
(`redonats`, `newdonats`, `beerstat`) already treat RabbitMQ as the shared bus, publishing
donations to a durable queue (`redonats` defaults to `donations`).

## Goals / Non-Goals

**Goals:**
- Publish each consumed donation to a durable `donations` queue with the minimum machinery that
  satisfies the `donation-publishing` spec.
- Keep the change additive to the existing listener loop (no CLI surface change) and consistent
  with the project's rustls-only TLS choices.

**Non-Goals:**
- Matching `redonats`' normalized `DonationCreate` payload — this CLI publishes the raw gateway
  JSON.
- Publisher confirms, batching, per-message headers, dead-lettering, or exchange declarations.
- Making the queue name or routing key configurable.
- Reworking the existing stdout/diagnostic split or the websocket reconnect logic.

## Decisions

**AMQP client: `lapin`, built with rustls *ring* only.** `lapin` is the de-facto async RabbitMQ
client for Tokio and the only one with first-class rustls support. Add it with default features
disabled and `features = ["default-runtime", "tokio", "rustls--ring",
"rustls-webpki-roots-certs"]`, so no `native-tls`/OpenSSL dependency is introduced.

The provider choice is load-bearing: `reqwest`'s `rustls-tls` already enables rustls' **`ring`**
provider (via its `__rustls-ring` feature), and at HEAD rustls carried only that one provider.
lapin's aggregate `rustls` feature would also switch on rustls' **`aws_lc_rs`** provider, leaving
rustls with two provider features — at which point `rustls::ClientConfig::builder()` (used by
`reqwest`/`tokio-tungstenite`) panics with "multiple crypto providers". Selecting lapin's
`rustls--ring` keeps rustls ring-only, so lapin's connector and the existing HTTP/websocket stack
agree on one provider. Verified with `cargo tree`: `aws-lc-rs`/`openssl-sys`/`native-tls` are
absent and rustls has only the `ring` feature. Alternatives considered: `amqprs` (less widely
used, more manual protocol handling) and hand-rolling the AMQP handshake (rejected as
disproportionate).

**The DSN requirement lives in the listen path, not `Config::load`.** `Config` gains a
`rabbit_mq_dsn: Option<String>` read like the other keys, but `login`/`token`/`refresh` do not
need a broker, so the panic must happen inside `listen`. `listen` takes the DSN with an
`expect`/`panic!` whose message names `RABBIT_MQ_DSN`, producing the required fail-fast with a
non-zero exit — before any gateway connection is attempted. Alternative considered: validating
in `Config::load` (rejected: it would make `login` panic for an unrelated missing key).

**A `RabbitPublisher` owns the connection and reconnects lazily.** New module `src/publish.rs`
holds the lapin `Connection` and a reusable `Channel`. Construction does no I/O; the first
publish (or a startup `preflight`, which `listen` calls so a broker problem is visible early)
connects, declares the durable queue `donations`, and enables publisher confirms. Each publish
goes to the default exchange (`""`) with routing key `donations` and
`BasicProperties::delivery_mode = 2`, then awaits the confirm. If a publish fails (or the broker
nacks), the publisher logs on stderr, drops the dead handles so the retry reconnects and
re-declares, and `publish_with_retry` retries the donation once — never panicking the loop.
Connection-level backoff is delegated to lapin (`ConnectionProperties::enable_auto_recover`)
rather than a hand-rolled sleep. This keeps a broker hiccup from either tearing down the donation
stream or silently dropping an event. Alternative considered: recreating a channel per publish
(simpler but a round-trip per donation) — rejected in favor of one reusable channel plus
reconnect.

**The gateway JSON `Value` is re-serialized for the message body.** The donation is already a
`serde_json::Value`; publishing `serde_json::to_vec(&donation)` keeps every field, including
empty/null ones, with no remapping. Note key *order* is not byte-preserved: `serde_json`'s
default map sorts keys, which is exactly what the existing stdout print already does — so
"verbatim" here means the same object content, consistent across stdout and the queue. Enabling
`preserve_order` was considered and rejected to avoid a global serialization change for no
semantic gain.

**Publishing is wired through the dense point the listener already has.** `drain`/`handle_text`
gain access to the publisher so a donation is published and printed at the same call site;
`listen` builds the publisher once (before the reconnect loop) and passes it down. Non-donation
frames (notices, keepalives) publish nothing, so the queue carries donations only.

## Risks / Trade-offs

- **`lapin` rustls feature/version mismatch** → Pin the dependency from `cargo add`/metadata and
  verify `cargo build` before writing the publisher body; fall back to another rustls feature
  variant if the first does not compile.
- **Broker down at startup** → Construction does no I/O; `listen` calls `preflight` and only
  warns on failure, then the first publish reconnects. If the broker stays unreachable, publishes
  fail loudly on stderr and are retried on later donations rather than silently dropped or
  aborting the stream.
- **Panic on missing DSN breaks existing `listen` deployments** → This is the requested
  behavior; `.env.example` and `README.md` document `RABBIT_MQ_DSN` so the failure is
  self-explanatory.
- **A slow broker stalls the read loop** → The single-task `select!` awaits the publish before
  reading the next frame; acceptable at donation volume, and the reconnect path bounds the stall
  to the backoff delay.
- **Duplicate delivery on retry** → A retried publish can duplicate a message if the broker
  accepted it before the failure was observed. Accepted: at-least-once is the right trade for a
  donation feed, and consumers of `donations` already tolerate it.

## Migration Plan

No data migration. Add `RABBIT_MQ_DSN` to the deployment `.env` before upgrading, or `listen`
will panic by design. Rollback is reverting the commit (the dropped `RABBIT_MQ_DSN` key becomes
inert). Verify with the unit tests for config, extraction, and the publisher's queue/payload
handling, plus the standing checks.
