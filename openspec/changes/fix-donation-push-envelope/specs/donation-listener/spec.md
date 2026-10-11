## Purpose

Define how the CLI listens to the DonationAlerts Centrifugo gateway and turns incoming
publications into donation output, so a subscribed listener emits every donation the user
makes and never drops one silently.

## ADDED Requirements

### Requirement: Donations are emitted as JSON lines

While subscribed to the user's donation channel, the listener SHALL write each received
donation to standard output as a single JSON object on its own line, and SHALL write its
connection and diagnostic messages to standard error so stdout carries only donation data.

#### Scenario: A donation arrives

- **WHEN** the gateway delivers a donation publication on the subscribed channel
- **THEN** the listener writes the donation payload as one JSON object followed by a newline
  on standard output

#### Scenario: Non-donation frames are not printed

- **WHEN** the connection receives command replies, subscription acknowledgements, or
  keepalive frames that carry no donation
- **THEN** nothing is written to standard output for those frames

### Requirement: The publication envelope is read correctly

A gateway push frame MAY carry the publication payload either under a `pub` object or, in
older protocol shapes, directly on the push. The listener SHALL locate the donation payload
under the `pub` envelope when present and SHALL fall back to a payload carried directly on the
push. Within the located payload, the donation SHALL be read whether it is delivered as a JSON
object or as a JSON-encoded string.

#### Scenario: Payload nested under the pub envelope

- **WHEN** a push frame has the shape `{"push": {"channel": "...", "pub": {"data": {...}}}}`
- **THEN** the listener prints the `data` donation as a JSON line

#### Scenario: Payload carried directly on the push

- **WHEN** a push frame has the shape `{"push": {"channel": "...", "data": {...}}}`
- **THEN** the listener prints that donation as a JSON line

#### Scenario: Payload delivered as an encoded string

- **WHEN** the located payload is a JSON string rather than an object
- **THEN** the listener decodes it and prints the resulting donation as a JSON line

### Requirement: An unusable push is reported, not dropped silently

When a push frame carries no recognizable publication payload, or a payload cannot be decoded,
the listener SHALL report the condition on standard error and continue listening, so a payload
shape mismatch is visible rather than appearing as an idle subscription.

#### Scenario: Push without a recognizable payload

- **WHEN** a push frame is received that has neither a `pub` envelope nor a direct payload
- **THEN** the listener writes a diagnostic naming the frame to standard error
- **AND** the listener keeps listening and writes nothing to standard output

#### Scenario: Undecodable payload

- **WHEN** a located payload cannot be decoded as JSON
- **THEN** the listener writes a diagnostic to standard error
- **AND** the listener keeps listening
