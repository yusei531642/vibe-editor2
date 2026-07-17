# Claude Agent sidecar JSONL protocol

Every line is one JSON object with `version: 1`.

- Rust request: `{ "type":"request", "id":"1", "method":"spawn", "params":{...} }`
- Sidecar response: `{ "type":"response", "id":"1", "ok":true, "result":{...} }`
- Sidecar error response: `{ "type":"response", "id":"1", "ok":false, "error":{ "code":"...", "message":"...", "recoverable":false } }`
- Event stream: `{ "type":"event", "event":{ "type":"messageDelta", ... } }`
- Startup handshake: `{ "type":"hello", "protocol":"vibe-claude-agent", "capabilities":[...] }`

Request methods are `spawn`, `resume`, `fork`, `turn`, `write`, `inject`, `steer`,
`interrupt`, `respondApproval`, `stop`, and `dispose`. Responses correlate solely by
`id`; events are independent and normalized by Rust before renderer delivery.

Credentials are never protocol fields. Rust passes the allowlisted credential variables
only in the child environment. Tool inputs, command argv, cwd, and blocked paths are not
included in event payloads.
