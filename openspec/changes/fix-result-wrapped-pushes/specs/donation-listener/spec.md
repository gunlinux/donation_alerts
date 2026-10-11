## Purpose

Define how the CLI listens to the DonationAlerts Centrifugo gateway and turns incoming
publications into donation output, so a subscribed listener emits every donation the user
makes and never drops one silently.

## ADDED Requirements

### Requirement: Donations are emitted as JSON lines

While subscribed to the user's donation channel, the listener SHALL write each received
donation to standard output as a single JSON object on its own line, and SHALL write its
connection and diagnostic messages to standard error, so stdout carries only donation data.

#### Scenario: A donation arrives

- **WHEN** the gateway delivers a donation publication on the subscribed channel
- **THEN** the listener writes the donation payload as one JSON object followed by a newline
  on standard output

#### Scenario: Non-donation frames are not printed

- **WHEN** the connection receives command replies, subscription acknowledgements, or
  subscription notices that carry no donation
- **THEN** nothing is written to standard output for those frames

### Requirement: The publication envelope is read correctly

The DonationAlerts gateway SHALL be assumed to wrap every server push in a top-level `result`
object (Centrifugo 2.x as deployed by DonationAlerts): `{"result":{"type":1,"channel":"…",
"data":{…}}}`. When the frame carries a `result` object, the listener SHALL locate the donation
at `result.data.data`. The listener SHALL also accept the raw Centrifugo shapes as a fallback —
`push.pub.data` and a payload carried directly on `push` — and SHALL read the located donation
whether it is delivered as a JSON object or as a JSON-encoded string.

#### Scenario: Donation under the result-wrapped publication

- **WHEN** a frame has the shape `{"result":{"type":1,"channel":"$alerts:donation_<id>",
  "data":{"seq":N,"data":{…donation…}}}}`
- **THEN** the listener prints the inner donation object as a JSON line

#### Scenario: Payload nested under the legacy pub envelope

- **WHEN** a push frame has the shape `{"push":{"channel":"…","pub":{"data":{…}}}}`
- **THEN** the listener prints the `data` donation as a JSON line

#### Scenario: Payload carried directly on the push

- **WHEN** a push frame has the shape `{"push":{"channel":"…","data":{…}}}`
- **THEN** the listener prints that donation as a JSON line

#### Scenario: Payload delivered as an encoded string

- **WHEN** the located payload is a JSON string rather than an object
- **THEN** the listener decodes it and prints the resulting donation as a JSON line

### Requirement: Non-donation pushes are ignored without noise

A `result`-wrapped frame whose payload is not a donation — such as the subscription notice
`{"result":{"type":1,"channel":"…","data":{"info":{…}}}}` — SHALL be recognized as an expected
non-donation frame and ignored: nothing on standard output and no diagnostic on standard error.

#### Scenario: Subscription notice is ignored

- **WHEN** a `result`-wrapped frame carries `data.info` and no donation object
- **THEN** nothing is written to standard output and no diagnostic is written to standard error

#### Scenario: Command replies are ignored

- **WHEN** a frame is a reply to the listener's own command (it carries an `id`)
- **THEN** nothing is written to standard output

### Requirement: An unusable push is reported, not dropped silently

When a frame is an actual publication but carries no recognizable donation payload, or a located
payload cannot be decoded, the listener SHALL report the condition on standard error and
continue listening, so a payload-shape mismatch is visible rather than appearing as an idle
subscription.

#### Scenario: Push without a recognizable payload

- **WHEN** a publication frame is received whose payload is neither a donation object nor
  decodable JSON
- **THEN** the listener writes a diagnostic naming the frame to standard error
- **AND** the listener keeps listening and writes nothing to standard output
