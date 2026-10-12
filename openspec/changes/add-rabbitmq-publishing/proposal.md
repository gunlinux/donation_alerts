## Why

`listen` can now read live donations off the DonationAlerts gateway, but it only prints each
one as a JSON line — a downstream consumer (a chat bot, a stats worker) has to scrape stdout to
react to a donation. The other services in this workspace are wired together by RabbitMQ
exchanges and queues, so the donation stream needs to join that bus instead of ending at a
terminal.

## What Changes

- `listen` **always** publishes each consumed donation to RabbitMQ, in addition to the existing
  stdout JSON line (stdout output is unchanged).
- Configuration gains the `RABBIT_MQ_DSN` key, read from `.env` / the process environment with
  the same precedence rules as the existing keys. `listen` **panics** with a clear message when
  `RABBIT_MQ_DSN` is unset or empty, since publishing is no longer optional.
- The donor's message is published **verbatim** — the donation JSON object exactly as the
  gateway sent it, with no re-mapping. Empty or null fields (`name`, `message`, `payin_system`,
  `shown_at`, …) are passed through untouched.
- Messages go to the default exchange under a fixed routing key/queue name: a durable queue
  named `donations`, published persistently (delivery mode 2) so a broker restart does not lose
  an event.
- The listener connects to the broker before subscribing to the gateway channel, declares the
  durable queue, and keeps publishing across its existing reconnect loop; a publish failure or
  a dropped broker connection is reported on stderr and recovered (reconnect + retry) rather
  than silently dropping a donation.
- Documentation (`.env.example`, `README.md`) documents the new key, the queue, and the panic.

No CLI surface changes (no new subcommand or flag); the only behavioral change is that `listen`
now also requires a broker and publishes to it.

## Capabilities

### New Capabilities
- `donation-publishing`: the contract for publishing each consumed donation to RabbitMQ — the
  required `RABBIT_MQ_DSN`, the fail-fast panic when it is missing, the payload (the raw
  gateway JSON), and the fixed durable `donations` queue with persistent delivery.

### Modified Capabilities
- `env-loading`: add `RABBIT_MQ_DSN` to the set of keys read with `.env` precedence over the
  inherited environment.

## Impact

- **Code**: new `src/publish.rs` (RabbitMQ publisher) and small edits in `src/config.rs`
  (read `RABBIT_MQ_DSN`) and `src/stream.rs` (publish alongside the stdout print).
- **Dependencies**: adds an AMQP client crate (`lapin`) built against rustls, matching the
  project's existing rustls-only TLS stack.
- **Config / docs**: `.env.example` and `README.md` gain the `RABBIT_MQ_DSN` key and the queue
  contract.
- **Behavioral**: `listen` now fails fast (panic) without `RABBIT_MQ_DSN`, and every received
  donation is published to `donations` in addition to being printed.
- **Related**: the sibling `redonats`/`newdonats` projects publish donations to RabbitMQ over
  FastStream; this change brings the Rust consumer onto the same bus. Interop with those
  consumers' exact payload schema is out of scope — the raw gateway JSON is published as-is.
