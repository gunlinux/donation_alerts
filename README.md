# donation-alerts

A tiny Rust consumer for [DonationAlerts](https://www.donationalerts.com): a CLI
that runs the OAuth2 authorization-code flow, then prints live donations and
publishes them to RabbitMQ.

```
login   -> open the URL, approve, copy ?code= from the redirect
token   -> exchange the code, store ACCESS_TOKEN in .env
refresh -> renew ACCESS_TOKEN from the stored refresh token
listen  -> connect to the Centrifugo gateway; print donations as JSON lines
           and publish each one to the durable `donations` RabbitMQ queue
```

## Configuration (`.env`)

| Variable         | Used by        | Description                                             |
| ---------------- | -------------- | ------------------------------------------------------- |
| `APP_ID`         | `login`/`token`| OAuth application id (client id)                        |
| `CLIENT_SECRET`  | `token`/`refresh` | OAuth application secret                             |
| `USER_ID`        | `listen`       | DonationAlerts user id; channel `$alerts:donation_<id>` |
| `REDIRECT_URI`   | `login`/`token`| Must match the app's registered redirect URI            |
| `SCOPES`         | `login`        | Space-separated scopes (sensible default provided)      |
| `ACCESS_TOKEN`   | `listen`       | Written by `token`/`refresh`; read by `listen`          |
| `RABBIT_MQ_DSN`  | `listen`       | RabbitMQ broker URL; **required** by `listen`           |

`listen` always publishes, so it **panics** with a message naming `RABBIT_MQ_DSN`
when that key is unset or empty — set it before running `listen`.

The OAuth **refresh token** and access-token expiry are kept in a separate
secrets file `.tokens.json` (gitignored, written with mode `0600`), never in
`.env`.

Values in `.env` take precedence over variables already exported in the shell
for the keys above, so a token written by `token`/`refresh` is always the one
used by the next run. A key that `.env` does not define still falls back to the
environment. Quote values that contain spaces (for example
`SCOPES="oauth-user-show oauth-donation-subscribe"`): an unquoted multi-word
value is a parse error, and the CLI stops with an error naming `.env`.

## Usage

```sh
cp .env.example .env      # fill in APP_ID, CLIENT_SECRET, USER_ID, RABBIT_MQ_DSN
cargo run -- login        # open the printed URL, approve, copy the ?code=
cargo run -- token <CODE> # stores ACCESS_TOKEN + refresh material
cargo run -- refresh      # renews ACCESS_TOKEN from the refresh token
cargo run -- listen       # prints each donation and publishes it to RabbitMQ
```

`listen` renews the access token before connecting when the stored expiry is
unknown or near (within 60s), and once more if the gateway rejects the token
(HTTP 401), so a long-running listener survives token expiry. If renewal is
impossible it exits with a message telling you to re-run `token <CODE>`.

`listen` reconnects on its own (5s backoff) and keeps the websocket alive with
a periodic ping.

Each donation is published to RabbitMQ as well as printed. Messages go to the
default exchange with the routing key `donations`, into a durable queue of the
same name, and are marked persistent (delivery mode 2) so a broker restart does
not lose them. The published body is the raw gateway JSON — empty or null fields
(`name`, `message`, …) and fields the CLI does not interpret are passed through
unchanged. The broker is declared at startup; if it is unreachable, the failure
is reported on stderr and republishing is retried on the next donation rather
than tearing down the stream.

DonationAlerts wraps every server push in a top-level `result` object
(Centrifugo 2.x), so each donation is read from `result.data.data`, for example
`{"result":{"type":1,"channel":"$alerts:donation_<id>","data":{"seq":N,"data":{…}}}}`.
The raw Centrifugo shapes (`push.pub.data`, then a flat `push.data`, and a
JSON-encoded string) are accepted as a fallback. The subscription notice the
gateway pushes right after subscribing carries `result.data.info` and is ignored.
A publication whose payload cannot be read is reported on stderr instead of being
dropped silently, so a payload-shape mismatch shows up in the log rather than
looking like an idle subscription.
