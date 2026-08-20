//! `AgileConfig` client for the [`config`] crate.
//!
//! This crate talks to an [AgileConfig](https://github.com/dotnetcore/AgileConfig)
//! cluster over HTTP and WebSocket and exposes configuration through two types:
//!
//! - [`Client`] loads published key/value items, optionally caches them on
//!   disk, and can keep a WebSocket session for live reload notifications.
//! - [`Source`] implements [`config::AsyncSource`] so the snapshot can be
//!   composed with other sources (files, environment variables, and so on).
//!
//! # Building a client
//!
//! Construct [`ClientOptions`] directly or use [`Client::builder`]:
//!
//! ```no_run
//! use agile_config_client::{Client, ClientOptions};
//!
//! let from_struct = Client::new(ClientOptions {
//!     app_id: "app".into(),
//!     secret: "secret".into(),
//!     nodes: vec!["http://localhost:5000".into()],
//!     env: "DEV".into(),
//!     ..ClientOptions::default()
//! })?;
//!
//! let from_builder = Client::builder()
//!     .app_id("app")
//!     .secret("secret")
//!     .nodes(["http://localhost:5000"])
//!     .env("DEV")
//!     .build()?;
//! # Ok::<(), agile_config_client::Error>(())
//! ```
//!
//! # One-shot load with `config`
//!
//! [`Source::collect`][source::Source] (via [`Client::source`]) performs HTTP
//! (and cache fallback). It does **not** open a WebSocket.
//!
//! ```no_run
//! use agile_config_client::{Client, ClientOptions};
//!
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Client::new(ClientOptions {
//!     app_id: "app".into(),
//!     secret: "secret".into(),
//!     nodes: vec!["http://localhost:5000".into()],
//!     ..ClientOptions::default()
//! })?;
//!
//! let settings = config::Config::builder()
//!     .add_async_source(client.source())
//!     .build()
//!     .await?;
//!
//! let connection = settings.get_string("db.connection")?;
//! # let _ = connection;
//! # Ok(())
//! # }
//! ```
//!
//! Keys that the C# client exposes as `group:key` become dotted paths for the
//! `config` crate (`db:connection` → `db.connection`).
//!
//! # Live updates
//!
//! Call [`Client::connect`] to pull configuration and start WebSocket
//! reconnect/heartbeat. Keep the `Client` alive, then listen with
//! [`Client::subscribe`]. This crate never rebuilds [`config::Config`] for you.
//!
//! ```no_run
//! use agile_config_client::{Client, ClientOptions};
//!
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Client::new(ClientOptions {
//!     app_id: "app".into(),
//!     secret: "secret".into(),
//!     nodes: vec!["http://localhost:5000".into()],
//!     ..ClientOptions::default()
//! })?;
//! client.connect().await?;
//!
//! let mut rx = client.subscribe();
//! while rx.changed().await.is_ok() {
//!     let snapshot = client.snapshot();
//!     // Rebuild `config::Config` or swap application state here.
//!     let _ = snapshot.get("db:connection");
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Lookups on [`ConfigSnapshot`] are case-sensitive.
//!
//! # Features
//!
//! | Feature | Default | Purpose |
//! | --- | --- | --- |
//! | `cache-encrypt` | off | AES-ECB encryption for the local cache file (C# compatible) |

#![warn(missing_docs)]

mod auth;
mod cache;
mod client;
mod error;
mod http;
mod nodes;
mod options;
mod protocol;
mod source;
mod store;
mod websocket;

pub use client::Client;
pub use error::Error;
pub use options::{CacheOptions, ClientBuilder, ClientOptions};
pub use protocol::ConfigItem;
pub use source::Source;
pub use store::ConfigSnapshot;
