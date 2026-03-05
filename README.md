# Simple Chat

An async chat server and CLI client built with Rust and Tokio.

## Architecture

```
simple-chat/
├── protocol/    Shared message types and newline-delimited JSON codec
├── server/      TCP chat server with per-connection tasks
└── client/      Interactive CLI client
```

The server uses `DashMap` for lock-free concurrent user management and bounded
`mpsc` channels for per-client write queues, providing natural backpressure.
Username uniqueness is enforced atomically via `DashMap::entry()`. Graceful
shutdown is handled through `CancellationToken` propagation.

## Build & Run

```bash
# Build
cargo build --all

# Start the server (default port 8080)
cargo run --bin server

# Or with a custom port
CHAT_PORT=9000 cargo run --bin server

# Connect a client
cargo run --bin client -- --host 127.0.0.1 --port 8080 --username alice

# Client commands:
#   send <message>    Send a message to the chat room
#   leave             Disconnect and exit
```

## Testing

```bash
cargo test --all
```

Tests include:
- Protocol encode/decode round-trips (unit)
- Join broadcast, message routing, leave notification (integration)
- Duplicate username rejection (integration)
- Disconnect cleanup (integration)
- Username validation — empty and oversized names (integration)

## Code Quality

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
```

## Git Hooks

To enable the pre-commit hook:

```bash
git config core.hooksPath .githooks
```

This runs `cargo fmt --check`, `cargo clippy`, and `cargo test` before each commit.
