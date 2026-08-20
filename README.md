# agile-config-client

A Rust client for [AgileConfig](https://github.com/dotnetcore/AgileConfig).

The crate pulls published configuration over HTTP, can keep a WebSocket session
for live reload notifications, and exposes an [`AsyncSource`](https://docs.rs/config)
for the [`config`](https://crates.io/crates/config) crate.

[![Crates.io](https://img.shields.io/crates/v/agile-config-client)](https://crates.io/crates/agile-config-client)
[![Documentation](https://docs.rs/agile-config-client/badge.svg)](https://docs.rs/agile-config-client)

More API detail is in the [crate documentation](https://docs.rs/agile-config-client).

## Install

```toml
[dependencies]
agile-config-client = "0.1"
config = { version = "0.15", features = ["async"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

AES encryption of the on-disk cache is optional and pulls extra crates (`aes`,
`ecb`, `sha1`). Enable it only when you set `cache.encrypt = true`:

```toml
agile-config-client = { version = "0.1", features = ["cache-encrypt"] }
```

## Build a client

`Client::new` takes a public [`ClientOptions`](https://docs.rs/agile-config-client/latest/agile_config_client/struct.ClientOptions.html)
struct. `Client::builder` fills the same struct and then calls `new`.

```rust
use agile_config_client::{Client, ClientOptions};

let from_struct = Client::new(ClientOptions {
    app_id: "app".into(),
    secret: "secret".into(),
    nodes: vec!["http://localhost:5000".into(), "http://localhost:5001".into()],
    env: "DEV".into(),
    name: Some("order-service".into()),
    tag: Some("canary".into()),
    ..ClientOptions::default()
})?;

let from_builder = Client::builder()
    .app_id("app")
    .secret("secret")
    .nodes(["http://localhost:5000"])
    .env("DEV")
    .build()?;
```

Required fields are `app_id` and at least one node URL. `secret` may be empty.
`env` is uppercased. Comma-separated node strings are split.

### Options

| Field | Default | Meaning |
| --- | --- | --- |
| `app_id` | empty (required) | Application id from the AgileConfig console |
| `secret` | empty | Application secret |
| `nodes` | empty (required) | Node base URLs (`http://` / `https://`) |
| `env` | empty | Target environment; empty lets the server choose |
| `name` / `tag` | `None` | Labels shown in the admin console |
| `http_timeout` | 100s | HTTP pull timeout |
| `reconnect_interval` | 5s | WebSocket reconnect delay |
| `heartbeat_interval` | 30s | WebSocket `ping` interval |
| `cache.enabled` | `true` | Persist the last successful pull |
| `cache.directory` | empty (cwd) | Directory for `{appId}.agileconfig.client.configs.cache` |
| `cache.encrypt` | `false` | AES-encrypt the cache file (C# compatible). Requires the `cache-encrypt` feature |

## Use with the `config` crate

`Client` does not implement `AsyncSource`. Call `client.source()` to get a
[`Source`](https://docs.rs/agile-config-client/latest/agile_config_client/struct.Source.html).
`collect` performs HTTP (and cache fallback) if needed. It does **not** open a
WebSocket.

```rust
let settings = config::Config::builder()
    .add_async_source(client.source())
    .build()
    .await?;

let connection = settings.get_string("db.connection")?;
```

AgileConfig items with a group become `group:key` in the snapshot (same as the
C# client) and `group.key` in `config` (`db:connection` → `db.connection`).
Lookups are case-sensitive.

## Live updates

Call `connect()` to pull configuration and start WebSocket heartbeat/reconnect.
Keep the `Client` alive. This crate never rebuilds `config::Config`; subscribe
and apply the new snapshot yourself.

```rust
client.connect().await?;

let settings = config::Config::builder()
    .add_async_source(client.source())
    .build()
    .await?;

let mut rx = client.subscribe();
while rx.changed().await.is_ok() {
    let snapshot = client.snapshot();
    // Rebuild `config::Config` or swap application state here.
    let _ = snapshot.get("db:connection");
}
```

A WebSocket failure is not fatal as long as HTTP or the local cache produced a
snapshot. Dropping the last `Client`/`Source` clone cancels background tasks.

## Examples

```bash
# One-shot HTTP pull
AGILE_CONFIG_APP_ID=app AGILE_CONFIG_SECRET=secret \
  AGILE_CONFIG_NODES=http://localhost:5000 AGILE_CONFIG_ENV=DEV \
  cargo run --example fetch

# WebSocket session and reload notifications
AGILE_CONFIG_APP_ID=app AGILE_CONFIG_SECRET=secret \
  AGILE_CONFIG_NODES=http://localhost:5000 \
  cargo run --example watch
```

## Differences from the C# client

- No static singleton and no injectable logger; use `tracing`
- No service registration or discovery in this version
- `config::Config` is a snapshot; reload is caller-owned via `subscribe()`
- Dictionary lookups are case-sensitive
