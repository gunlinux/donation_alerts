## Context

See proposal.md — Why. The listener already connects, handshakes, obtains a subscription token,
and subscribes; only frame interpretation is wrong. A live capture against the real gateway
(Centrifugo 2.5.1, channel `$alerts:donation_5116810`) returned:

```
connect reply      {"id":1,"result":{"client":"…","version":"2.5.1"}}
subscribe reply    {"id":2,"result":{"recoverable":true,"seq":12,"epoch":"…","offset":12}}
subscription push  {"result":{"type":1,"channel":"$alerts:donation_5116810","data":{"info":{…}}}}
```

The sibling project `newdonats` captured a donation frame from the same account as
`{"result":{"channel":"$alerts:donation_…","data":{"seq":227,"data":{…donation…,"recipient":
{"user_id":5116810}}}}}`. So command replies carry an `id`; server pushes do not.

## Goals / Non-Goals

**Goals:**
- Make the listener emit donations from the `result`-wrapped envelope the gateway actually sends.
- Keep the parser tolerant of the raw Centrifugo shapes so the same binary works if the gateway
  ever returns to `push`-wrapped frames (and for reuse/testing).

**Non-Goals:**
- Changing connection, handshake, subscription, token, or reconnect logic.
- Remapping/normalizing the donation JSON (the listener prints the payload verbatim).
- Fixing the reference project `redonats`, which shares the same wrong `push` assumption.

## Decisions

**Recognize the DA `result` envelope as a primary shape, `push` as a fallback.** The current
`extract_donation` starts from `frame.get("push")` and returns `Ok(None)` (a silent "not a push")
for anything else. Change it to classify a frame by shape:

- the frame has an `id` → it is a command reply → ignore;
- the frame has a top-level `result` with a `channel` → it is a DA server push: the donation is
  `result.data.data` when `result.data.data` is an object; otherwise (e.g. `result.data.info`)
  it is the subscription notice and is ignored without noise;
- otherwise fall back to the existing `push.pub.data` / `push.data` handling.

Rationale: the DA push is a `result` object, not a `push` object, so no amount of `push.*`
fallbacking can find it — the top-level key must be recognized. Keeping the `push` handling
costs nothing and preserves compatibility. Alternative considered: rewrite to a strict
DA-only parser; rejected because it discards working tolerance the current tests exercise.

**Distinguish "expected non-donation" from "malformed publication" by structure.** The
subscription notice is a normal event that must not spam stderr; a genuinely unrecognized
publication should still be reported. Rule: a `result` frame with a `channel` but no usable
`data.data` is the notice (silent); a payload that is present but not an object / not decodable
JSON is reported. This refines the existing "reported, not dropped silently" requirement.

## Risks / Trade-offs

- **Envelope mis-identification** → The rule keys on the top-level `result` object, verified
  live and against `newdonats`' captured frame; scenarios in the spec pin the shape.
- **Ignoring a real donation** → A donation is only ignored when `result.data.data` is absent or
  not an object; the acceptance check is a real captured frame with a donation.
- **Over-reporting the notice** → The subscription notice is silenced by the `data.info` shape,
  with a scenario asserting no stderr output.

## Migration Plan

Single-binary fix; no data or config migration. Verify with the existing test suite and a short
`listen` run against the live gateway; rollback is reverting the commit.
