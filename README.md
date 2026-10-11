# donation-alerts

A tiny Rust consumer for [DonationAlerts](https://www.donationalerts.com): a CLI
that runs the OAuth2 authorization-code flow and then prints live donations.

```
login   -> open the URL, approve, copy ?code= from the redirect
token   -> exchange the code, store ACCESS_TOKEN in .env
listen  -> connect to the Centrifugo gateway, print donations as JSON lines
```

## Configuration (`.env`)

| Variable         | Used by        | Description                                             |
| ---------------- | -------------- | ------------------------------------------------------- |
| `APP_ID`         | `login`/`token`| OAuth application id (client id)                        |
| `CLIENT_SECRET`  | `token`        | OAuth application secret                                |
| `USER_ID`        | `listen`       | DonationAlerts user id; channel `$alerts:donation_<id>` |
| `REDIRECT_URI`   | `login`/`token`| Must match the app's registered redirect URI            |
| `SCOPES`         | `login`        | Space-separated scopes (sensible default provided)      |
| `ACCESS_TOKEN`   | `listen`       | Written by `token`; read by `listen`                    |

## Usage

```sh
cp .env.example .env      # fill in APP_ID, CLIENT_SECRET, USER_ID
cargo run -- login        # open the printed URL, approve, copy the ?code=
cargo run -- token <CODE> # stores ACCESS_TOKEN in .env
cargo run -- listen       # prints each donation as one JSON object per line
```

`listen` reconnects on its own (5s backoff) and keeps the websocket alive with
a periodic ping.
