## 1. Fix frame interpretation

- [x] 1.1 In `src/stream.rs`, extend `extract_donation` so a frame with a top-level `result` and a `channel` is treated as a DonationAlerts push: return the donation at `result.data.data` when it is an object, treat the subscription notice (`result.data` present but no object `data.data`) as `Ok(None)`, and leave the existing `push.pub.data` / `push.data` handling as the fallback; verify `cargo test extract` (the extraction unit tests) passes
- [x] 1.2 In `handle_text`, ensure a frame carrying an `id` (a command reply) is ignored and that a `result`-wrapped subscription notice produces neither stdout nor a stderr diagnostic, while a publication with an unusable payload is still reported on stderr; verify the corresponding unit tests pass

## 2. Tests

- [x] 2.1 Add `result`-wrapped extraction tests using the captured frame: a donation at `result.data.data` is extracted, and a subscription notice (`result.data.info`) yields `Ok(None)`; verify `cargo test` passes
- [x] 2.2 Keep/add coverage proving the legacy `push.pub.data`, `push.data`, and JSON-encoded-string shapes still extract, and that an undecodable payload is reported; verify `cargo test` passes

## 3. Documentation

- [x] 3.1 Update `README.md` to state that the gateway wraps pushes in a top-level `result` object (donation at `result.data.data`), replacing the current `push.pub.data` description

## 4. Verification

- [x] 4.1 Run the standing checks and confirm all are clean: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`
- [x] 4.2 Run `cargo run -- listen` against the live gateway and confirm the subscription notice is not printed to stdout and that a received test donation is printed as one JSON line — live `listen` stayed subscribed with stdout at 0 bytes (notice not printed) and one stderr line; the donation-print half could not be observed live (no donor available on 2026-10-11) and is covered by the captured-frame unit test `donation_under_the_result_envelope_is_extracted`
