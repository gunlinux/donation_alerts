## Why

`listen` connects, subscribes, and prints "subscribed to …; waiting for donations", but no
donation is ever emitted. The gateway DonationAlerts runs (Centrifugo **2.5.1**) wraps *every*
server push in a top-level `result` object — the subscription notice and the donations alike —
as `{"result":{"type":1,"channel":"$alerts:donation_<id>","data":{"seq":N,"data":{…}}}}`. The
listener's parser only recognizes a top-level `push` key, so it classifies each of those frames
as "not a push" and drops it silently. The listener is deaf with no diagnostic.

The previous change (`fix-donation-push-envelope`) misdiagnosed this as a `push.pub` vs
`push.data` difference and fixed nothing. The authoritative reference is the older sibling
project `newdonats`, whose commit `03c90f1 "fix(getter): parse result-wrapped centrifugo
donation pushes"` captures a real frame from this same account (`recipient.user_id=5116810`)
and reads the donation from `result.data.data`.

## What Changes

- Recognize the DonationAlerts `result`-wrapped push: a frame whose top level is `result` (no
  `id`) carrying `channel` and `data`; the donation lives at `result.data.data`.
- Treat the `result`-wrapped subscription notice (`result.data.info`, no donation) as an
  expected non-donation frame and ignore it on stdout.
- Keep accepting the raw Centrifugo shapes (`push.pub.data`, `push.data`, and a payload carried
  as a JSON-encoded string) as fallbacks, so the parser stays robust to either envelope.
- Keep reporting an unexpected/unusable push on stderr instead of dropping it silently.
- Add unit tests over the captured `result`-wrapped frame (donation, subscription notice, and a
  malformed push), replacing the tests that asserted the wrong `push.pub` shape.
- Update `README.md` to document the `result`-wrapped envelope the gateway actually sends.

Not a BREAKING change: the CLI surface, commands, and configuration are unchanged.

## Capabilities

### New Capabilities
- `donation-listener`: the contract for connecting to the DonationAlerts Centrifugo gateway,
  subscribing to the user's donation channel, and emitting each received donation as one JSON
  line on stdout — including which envelope carries the payload and how non-donation frames are
  handled.

### Modified Capabilities
<!-- none: env-loading and token-refresh requirements are unchanged -->

## Impact

- **Code**: `src/stream.rs` — `extract_donation`/`handle_text` extraction and its tests only;
  connection, handshake, subscription, and reconnect logic are untouched.
- **Docs**: `README.md` gains a corrected note on the push envelope.
- **Data / config / dependencies**: none. No new crates; no `.env`, spec, or CLI changes.
- **Behavioral**: donations now appear on stdout; the subscription notice is ignored; an
  unexpected push is reported on stderr rather than dropped in silence.
- **Related**: the reference project `redonats` uses the same wrong `push` assumption and would
  ignore these frames too; its envelope handling is out of scope for this change.
