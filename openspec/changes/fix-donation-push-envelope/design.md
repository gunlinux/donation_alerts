## Context

See `proposal.md` for motivation. Relevant current state:

- `src/stream.rs::run_once` performs the full connection sequence (fetch connection token,
  handshake, request subscription token, subscribe) and then calls `drain`, which prints via
  `handle_text`. All of that connection and subscription logic works — the listener reaches
  "subscribed to …; waiting for donations" — so the defect is confined to extraction.
- `handle_text` currently does: `frame.get("push")`, then `push.get("data")`, then optionally
  parses a string, then prints. A push whose payload is nested under `push.pub` yields `None`
  at `push.get("data")` and the function returns early — silently.
- The gateway is Centrifugo (`WS_URL = wss://centrifugo.donationalerts.com/connection/websocket`),
  whose push frames wrap the publication under `pub`.
- The unit-test module in `stream.rs` is the existing pattern for testing pure helpers
  (`handle_text` is a pure `fn(&str)` apart from the print, so it is testable in place).

## Goals / Non-Goals

**Goals:**
- Read the donation out of the envelope the gateway actually sends, while staying tolerant of
  the shape the code already assumed.
- Make a future payload mismatch visible on stderr instead of silent.

**Non-Goals:**
- Any change to connection, handshake, subscription, token handling, or reconnect behavior.
- Adding a verbose/raw-frame dump flag or a `--json` schema — out of scope unless the user asks.
- Reformatting or filtering the donation payload; the existing "print it as-is" contract stands.
- Changing what `listen` writes to stdout vs stderr beyond the diagnostic requirement.

## Decisions

**1. Prefer `push.pub.data`, fall back to `push.data`.** The extraction becomes: locate the push
object; then take `push.pub.data` if a `pub` object exists, otherwise `push.data`. This matches
the Centrifugo envelope the gateway sends and keeps the previously-assumed shape working, so the
fix cannot regress if a deployment ever emits the flatter form.
*Alternatives considered:* (a) read only `pub.data` — simplest, but drops compatibility and any
non-gateway test fixture using the old shape; (b) normalize by recursively searching for `data`
— over-engineered for a two-shape problem.

**2. Keep the object-or-encoded-string tolerance.** The located payload is printed directly if
it is already a JSON value and decoded first if it is a string. This behavior already exists for
the legacy path and is preserved for the `pub` path; it costs one match arm.

**3. Report an unmatched push on stderr instead of returning silently.** When a push object
exists but no payload is found under either location, emit a one-line `eprintln!` that names the
channel and the frame. This is the diagnostic that turns "deaf listener with a successful
subscribe" into an actionable message. Non-push frames (command replies, keepalives) continue to
be ignored without output, since they are expected and not donations.

**4. Test `handle_text` through a captured-output seam.** The current function prints inline.
To assert emitted lines, tests capture output the same way the crate already does elsewhere
(or the function is refactored to a small pure extractor returning `Option<Value>` plus a thin
printing wrapper, keeping the existing `handle_text(raw: &str)` call site in `drain`). The pure
extractor is preferred: it makes the envelope rules unit-testable without capturing stdout.

## Risks / Trade-offs

- [Assuming the "correct" envelope without a live capture] → The fix accepts both `pub.data` and
  `data`, so it is correct under either shape; the new "unmatched push" diagnostic surfaces the
  real frame if a third shape exists, rather than failing silently again.
- [Adding an `eprintln!` per unmatched push could be noisy if the gateway sends frequent
  non-donation pushes] → The diagnostic fires only when a `push` object is present but its
  payload is unrecognizable — not for command replies or keepalives — so it stays quiet in
  normal operation.
- [Refactoring `handle_text` into extractor + printer touches a hot path] → The change is
  mechanical and covered by the four new scenario tests plus the existing suite.
- [Test donations made through the provider's "test alert" UI may not traverse the gateway at
  all] → The code fix is required regardless; the stderr diagnostic is what will distinguish
  "payload shape we don't parse" from "the provider never published" on the next real attempt.

## Migration Plan

1. Land the extraction fix and its unit tests; no data, config, or CLI migration.
2. Re-run `listen` and trigger a test donation; a printed JSON line confirms the envelope, and
   an "unrecognized push" stderr line now identifies any remaining shape mismatch.
3. Rollback is a plain revert — the change touches one function and its tests.
