## Why

`listen` connects, subscribes, and reports "subscribed to …; waiting for donations", yet real
test donations arrive and nothing is ever printed. The push parser reads the donation payload
from `push.data`, but the Centrifugo gateway DonationAlerts runs sends publications under the
`push.pub.data` envelope. Since `push.data` is absent, every donation takes the "ignore
everything else" branch and is dropped silently — the listener is deaf with no diagnostic.

## What Changes

- Fix donation extraction in `src/stream.rs::handle_text` so a push publication is read from
  `push.pub.data` (the shape the gateway actually sends) instead of `push.data`.
- Keep tolerating the legacy `push.data` shape as a fallback, and keep accepting both an object
  payload and a JSON-encoded string, so the fix is forward- and backward-compatible.
- Stop failing silently: when a `push` frame carries no recognizable publication payload, log a
  one-line diagnostic to stderr so a future mismatch is visible instead of looking like an idle
  listener.
- Add unit tests over captured-shaped frames: `pub.data` object, legacy `data` object,
  string-encoded payload, and an unrecognized push (no print, diagnostic emitted).
- Update `README.md` to note the payload envelope the listener expects.

Not a BREAKING change: the CLI surface, commands, and configuration are unchanged.

## Capabilities

### New Capabilities
- `donation-listener`: the contract for connecting to the DonationAlerts Centrifugo gateway,
  subscribing to the user's donation channel, and emitting each received donation as one JSON
  line on stdout — including which envelope carries the payload and how unrecognized frames are
  handled.

### Modified Capabilities
<!-- none: env-loading and token-refresh requirements are unchanged -->

## Impact

- **Code**: `src/stream.rs` (`handle_text` extraction and its tests only; connection,
  handshake, subscription, and reconnect logic are untouched).
- **Docs**: `README.md` gains a short note on the push envelope.
- **Data / config / dependencies**: none. No new crates; no `.env`, spec, or CLI changes.
- **Behavioral**: donations now appear on stdout; a push frame with no payload is reported on
  stderr rather than dropped in silence.
