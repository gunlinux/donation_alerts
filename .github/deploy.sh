#!/bin/bash
# =============================================================================
# donation-alerts — deploy the listener container image from Docker Hub.
#
# Runs ON the server, invoked by .github/workflows/deploy.yaml over SSH with
# DEPLOY_TAG=<commit short SHA> exported. The image is built and pushed in CI
# (public repo gunlinuxloki/donation-alerts — no docker login needed here); this
# script pulls that exact tag, installs the systemd unit with the tag baked in,
# and (re)starts the service. Idempotent: safe to re-run.
#
# Unlike gunlinux.ru there is no nginx, no CSS bundle and no legacy service to
# swap: the listener writes nothing but its own log, so the whole deploy is
# "pull the tag, pin the unit, restart".
#
# Prerequisite (one-time, on the server): a hand-written .env at $REPO_DIR
# carrying APP_ID and RABBIT_MQ_DSN, plus a .tokens.json produced by
# `donationalerts token <CODE>`. Both are gitignored and survive
# `git reset --hard`. The checkout itself is bootstrapped by this script.
# =============================================================================
set -euo pipefail

REPO_DIR="/home/loki/www/donation_alerts"
BRANCH="main"
IMAGE="gunlinuxloki/donation-alerts"
UNIT="donation-alerts"

TAG="${DEPLOY_TAG:?DEPLOY_TAG is required (commit short SHA)}"

# --- 1. Pull the deployed commit ----------------------------------------------
# Bootstrap the checkout if the directory is not a clone yet (fresh host, or a
# directory whose secrets were copied by hand). HTTPS because the repo is
# public: no deploy key needed on the server. `reset --hard` never touches
# untracked files, so a pre-existing .env / .tokens.json survives.
mkdir -p "$REPO_DIR"
cd "$REPO_DIR" || exit 1
if [ ! -d .git ]; then
    git init -q
    git remote add origin https://github.com/gunlinux/donation_alerts.git
fi
git remote set-url origin https://github.com/gunlinux/donation_alerts.git

git fetch --all
git reset --hard "origin/$BRANCH"

# Guard: refuse to run against a commit that lacks the deploy artifacts.
if [ ! -f deploy/donation-alerts.service ]; then
    echo "ERROR: origin/$BRANCH lacks deploy/donation-alerts.service — refusing to deploy." >&2
    exit 1
fi

# --- 2. Guard the runtime secrets ---------------------------------------------
# `listen` panics without RABBIT_MQ_DSN, which would leave the unit in a
# restart loop; fail the deploy instead of shipping a broken service.
if [ ! -f .env ]; then
    echo "ERROR: $REPO_DIR/.env is missing (APP_ID, RABBIT_MQ_DSN, ...) — refusing to deploy." >&2
    exit 1
fi
if ! grep -qE '^ *RABBIT_MQ_DSN=.+' .env; then
    echo "ERROR: RABBIT_MQ_DSN is unset in .env — refusing to deploy." >&2
    exit 1
fi
# Note: do NOT strip quotes from .env here (gunlinux.ru does, for its legacy
# values). SCOPES is legitimately quoted and multi-word; unquoting it makes
# dotenvy fail to parse the file.

if ! grep -qE '^ *ACCESS_TOKEN=.+' .env && [ ! -s .tokens.json ]; then
    echo "WARNING: no ACCESS_TOKEN in .env and no .tokens.json — the listener" >&2
    echo "         cannot connect until you run 'donationalerts token <CODE>'." >&2
fi

# --- 3. Install the unit with the tag and the run uid baked in ----------------
UID_GID="$(id -u):$(id -g)"
sed -e "s|@IMAGE_TAG@|$TAG|g" -e "s|@RUN_UID@|$UID_GID|g" deploy/donation-alerts.service \
    | sudo tee "/etc/systemd/system/$UNIT.service" >/dev/null
sudo systemctl daemon-reload

# --- 4. (Re)start the listener ------------------------------------------------
docker pull "$IMAGE:$TAG" >/dev/null
sudo systemctl enable --now "$UNIT" >/dev/null

# `enable --now` starts the unit when it is not already running (first deploy,
# or a unit left inactive by an earlier failure) but never restarts an active
# one; restart explicitly when the running image differs from the deployed tag
# (idempotent no-op when the tag is unchanged).
CURRENT="$(docker inspect --format '{{.Config.Image}}' "$UNIT" 2>/dev/null || true)"
if [ "$CURRENT" != "$IMAGE:$TAG" ]; then
    sudo systemctl restart "$UNIT"
fi

# The listener holds a websocket instead of serving HTTP, so "up" means only
# that the container is still alive — the donation stream is checked in the log.
sleep 3
if ! docker ps --format '{{.Names}}' | grep -qx "$UNIT"; then
    echo "WARNING: container $UNIT is not running — check: journalctl -u $UNIT" >&2
fi

echo "Deployment completed: $IMAGE:$TAG"
echo "Follow the stream with: sudo journalctl -u $UNIT -f"
