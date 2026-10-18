# syntax=docker/dockerfile:1
#
# Container image for the DonationAlerts listener (the `donationalerts` binary).
#
# Built and pushed to Docker Hub by .github/workflows/deploy.yaml; the server
# run by .github/deploy.sh only `docker pull`s the tagged image. Build locally
# with (from the repo root):
#
#   docker build -t gunlinuxloki/donation-alerts:dev .
#
# Runtime config is NOT baked in: `.env` and `.tokens.json` are gitignored and
# stay on the host. The systemd unit (deploy/donation-alerts.service) mounts the
# deploy checkout at /data, which is this image's WORKDIR, so the app finds —
# and can rewrite — both files there.

# ---------------------------------------------------------------- builder --
# Alpine builder: the host is musl, so `cargo build --release` yields a
# self-contained binary with no cross-compilation. gcc + musl-dev are needed
# only for `ring` (C/asm sources pulled in by rustls via lapin/reqwest).
FROM rust:1.96-alpine AS builder

WORKDIR /build

RUN apk add --no-cache gcc musl-dev

# Cache-friendly dependency layer: copy the manifests first and build against a
# stub main, so the dependency tree compiles once and is reused as a Docker
# layer when only the crate sources change.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

COPY src ./src
# `touch`: COPY preserves mtimes, and cargo decides on mtime first.
RUN touch src/main.rs && cargo build --release --locked

# ---------------------------------------------------------------- runtime --
FROM alpine:3.20

# webpki-roots are compiled into the binary (rustls), so this is belt and braces
# for anything a future dependency resolves through the system store.
RUN apk add --no-cache ca-certificates

COPY --from=builder /build/target/release/donationalerts /usr/local/bin/donationalerts

# Where the unit bind-mounts the deploy checkout (.env, .tokens.json).
WORKDIR /data

ENTRYPOINT ["donationalerts"]
CMD ["listen"]
