## 1. Envelope extraction

- [x] 1.1 In `src/stream.rs`, refactor the payload extraction out of `handle_text` into a pure helper that returns the donation `Value` (or nothing), locating the payload under `push.pub.data` when a `pub` object is present and falling back to `push.data`; verify `cargo build` succeeds with no unused warnings
- [x] 1.2 Keep the object-or-JSON-string tolerance in the helper (decode a located string payload, pass through an object) and keep `drain`'s call site printing each extracted donation as one JSON line on stdout; verify `cargo build` succeeds
- [x] 1.3 When a `push` object is present but no payload is found under either location, emit a one-line diagnostic to stderr naming the channel and frame, and continue listening; verify the function still ignores command replies and keepalives without output

## 2. Tests

- [x] 2.1 Unit-test the extractor with a `{"push":{"channel":"...","pub":{"data":{...}}}}` frame and verify the `data` object is returned
- [x] 2.2 Unit-test the legacy `{"push":{"channel":"...","data":{...}}}` frame and verify that donation is returned
- [x] 2.3 Unit-test a frame whose located payload is a JSON-encoded string and verify it is decoded
- [x] 2.4 Unit-test an unmatched push (no `pub`, no direct payload) and a non-push frame, verifying neither yields a donation and the unmatched push is reported on stderr while the non-push frame is silent
- [x] 2.5 Run `cargo test` and verify all new and existing tests pass

## 3. Documentation and checks

- [x] 3.1 Add a short `README.md` note stating which push envelope the listener reads (`pub.data`, with `data` fallback) and that unrecognized pushes are reported on stderr
- [x] 3.2 Run the full check suite — `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` — and verify all pass
