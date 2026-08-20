//! Long-lived `AgileConfig` client: HTTP pull, cache, and WebSocket updates.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace, warn};

use crate::cache::{read_cache, write_cache};
use crate::error::Error;
use crate::http::fetch_config;
use crate::nodes::RandomNodes;
use crate::options::{ClientBuilder, ClientOptions};
use crate::protocol::ConfigItem;
use crate::source::Source;
use crate::store::{ConfigSnapshot, empty_snapshot};
use crate::websocket::{self, WsSink};

/// Client for an `AgileConfig` cluster.
///
/// `Client` owns HTTP loading, the optional local cache, and the WebSocket
/// session used for live reload notifications. Obtain a [`Source`] with
/// [`Self::source`] to integrate with the [`config`] crate.
///
/// Keep at least one `Client` clone alive for as long as you want the
/// WebSocket connection to stay up. The last drop cancels background tasks.
#[derive(Clone, Debug)]
pub struct Client {
    inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) options: ClientOptions,
    http: reqwest::Client,
    store: watch::Sender<Arc<ConfigSnapshot>>,
    loaded: AtomicBool,
    pub(crate) cancel: CancellationToken,
    session: Mutex<CancellationToken>,
    pub(crate) writer: tokio::sync::Mutex<Option<WsSink>>,
    pub(crate) loops_started: AtomicBool,
    pub(crate) reconnect_enabled: AtomicBool,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Inner")
            .field("app_id", &self.options.app_id)
            .field("loaded", &self.loaded)
            .finish_non_exhaustive()
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.reconnect_enabled.store(false, Ordering::SeqCst);
        self.cancel.cancel();
    }
}

impl Client {
    /// Creates a client from an options struct.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmptyAppId`] or [`Error::EmptyNodes`] when required
    /// fields are missing.
    pub fn new(options: ClientOptions) -> Result<Self, Error> {
        let options = options.normalized()?;
        let http = reqwest::Client::builder()
            .timeout(options.http_timeout)
            .build()?;
        let (store, _) = watch::channel(empty_snapshot());
        let cancel = CancellationToken::new();
        let session = cancel.child_token();
        session.cancel();
        Ok(Self {
            inner: Arc::new(Inner {
                options,
                http,
                store,
                loaded: AtomicBool::new(false),
                cancel,
                session: Mutex::new(session),
                writer: tokio::sync::Mutex::new(None),
                loops_started: AtomicBool::new(false),
                reconnect_enabled: AtomicBool::new(false),
            }),
        })
    }

    /// Starts a builder that constructs [`ClientOptions`] then this client.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Returns the normalized options used by this client.
    #[must_use]
    pub fn options(&self) -> &ClientOptions {
        &self.inner.options
    }

    /// Returns an [`AsyncSource`](config::AsyncSource) bound to this client.
    #[must_use]
    pub fn source(&self) -> Source {
        Source::new(Arc::clone(&self.inner))
    }

    /// Returns the latest configuration snapshot.
    #[must_use]
    pub fn snapshot(&self) -> Arc<ConfigSnapshot> {
        self.inner.snapshot()
    }

    /// Subscribes to snapshot updates.
    ///
    /// The library never rebuilds a [`config::Config`] value. After receiving
    /// a change, the caller should snapshot again and apply it to application
    /// state.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<Arc<ConfigSnapshot>> {
        self.inner.store.subscribe()
    }

    /// Pulls configuration via HTTP, falling back to the local cache.
    ///
    /// # Errors
    ///
    /// Returns [`Error::LoadFailed`] when every node fails and no cache exists.
    pub async fn load(&self) -> Result<(), Error> {
        self.inner.load().await
    }

    /// Loads configuration and starts the WebSocket session.
    ///
    /// A WebSocket failure is not fatal; the method still succeeds when HTTP
    /// or the cache produced a snapshot. Background reconnect and heartbeat
    /// loops keep running until [`Self::disconnect`] or the last clone is
    /// dropped.
    ///
    /// # Errors
    ///
    /// Returns [`Error::LoadFailed`] when no configuration could be obtained.
    pub async fn connect(&self) -> Result<(), Error> {
        self.inner.reconnect_enabled.store(true, Ordering::SeqCst);
        websocket::spawn_background_loops(&self.inner);
        if let Err(error) = self.inner.connect_websocket().await {
            warn!(
                ?error,
                "websocket connect failed; continuing with HTTP load"
            );
        }
        self.inner.load().await
    }

    /// Stops reconnecting and closes the WebSocket.
    pub async fn disconnect(&self) {
        self.inner.disconnect().await;
    }
}

impl Inner {
    pub(crate) fn snapshot(&self) -> Arc<ConfigSnapshot> {
        Arc::clone(&self.store.borrow())
    }

    pub(crate) async fn ensure_loaded(&self) -> Result<(), Error> {
        if self.loaded.load(Ordering::SeqCst) {
            return Ok(());
        }
        self.load().await
    }

    pub(crate) async fn load(&self) -> Result<(), Error> {
        let mut last_error: Option<Error> = None;
        for node in RandomNodes::new(&self.options.nodes) {
            match fetch_config(&self.http, &node, &self.options).await {
                Ok(payload) => match parse_items(&payload.json) {
                    Ok(items) => {
                        self.apply_snapshot(items, payload.publish_time_line_id, false);
                        if let Err(error) = write_cache(
                            &self.options.cache,
                            &self.options.app_id,
                            &self.options.secret,
                            &payload.json,
                        ) {
                            warn!(
                                ?error,
                                "client try to cache all configs to local but failed"
                            );
                        }
                        if let Err(error) = websocket::send_text(self, "loaded").await {
                            debug!(?error, "client try to send loaded msg to server but failed");
                        } else {
                            trace!("client send loaded to server by websocket");
                        }
                        return Ok(());
                    }
                    Err(error) => {
                        warn!(?error, node, "invalid configuration payload");
                        last_error = Some(error);
                    }
                },
                Err(error) => {
                    warn!(
                        ?error,
                        node, "client try to load all the configs but failed"
                    );
                    last_error = Some(error);
                }
            }
        }

        match self.load_from_cache() {
            Ok(()) => {
                trace!("client load all configs from local file");
                Ok(())
            }
            Err(cache_error) => {
                debug!(?cache_error, "local cache unavailable");
                Err(last_error.unwrap_or(Error::LoadFailed))
            }
        }
    }

    fn load_from_cache(&self) -> Result<(), Error> {
        let Some(json) = read_cache(
            &self.options.cache,
            &self.options.app_id,
            &self.options.secret,
        )?
        else {
            return Err(Error::LoadFailed);
        };
        let items = parse_items(&json)?;
        self.apply_snapshot(items, None, true);
        Ok(())
    }

    fn apply_snapshot(
        &self,
        items: Vec<ConfigItem>,
        publish_time_line_id: Option<String>,
        from_cache: bool,
    ) {
        let snapshot = Arc::new(ConfigSnapshot::from_items(
            items,
            publish_time_line_id,
            from_cache,
        ));
        self.store.send_replace(snapshot);
        self.loaded.store(true, Ordering::SeqCst);
    }

    pub(crate) async fn connect_websocket(self: &Arc<Self>) -> Result<(), Error> {
        let mut last_error = Error::LoadFailed;
        for node in RandomNodes::new(&self.options.nodes) {
            match websocket::connect(&node, &self.options).await {
                Ok(stream) => {
                    self.install_socket(stream).await;
                    trace!(node, "client connect websocket successful");
                    return Ok(());
                }
                Err(error) => {
                    warn!(?error, node, "client try to connect server occur error");
                    last_error = error;
                }
            }
        }
        Err(last_error)
    }

    async fn install_socket(self: &Arc<Self>, stream: websocket::WsStream) {
        let session = self.cancel.child_token();
        if let Ok(mut current) = self.session.lock() {
            current.cancel();
            *current = session.clone();
        }
        websocket::close_writer(self).await;
        let (sink, reader) = stream.split();
        *self.writer.lock().await = Some(sink);
        websocket::spawn_reader(Arc::downgrade(self), session, reader);
    }

    pub(crate) async fn disconnect(&self) {
        self.reconnect_enabled.store(false, Ordering::SeqCst);
        if let Ok(session) = self.session.lock() {
            session.cancel();
        }
        websocket::close_writer(self).await;
    }

    pub(crate) async fn handle_inbound(&self, text: &str) {
        websocket::handle_inbound(self, text).await;
    }
}

fn parse_items(json: &str) -> Result<Vec<ConfigItem>, Error> {
    Ok(serde_json::from_str(json)?)
}

#[cfg(test)]
mod tests {
    use super::Client;
    use crate::options::{CacheOptions, ClientOptions};

    #[test]
    fn new_rejects_empty_app_id() {
        let error = Client::new(ClientOptions {
            nodes: vec!["http://localhost:5000".into()],
            ..ClientOptions::default()
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "app_id must not be empty");
    }

    #[test]
    fn new_rejects_empty_nodes() {
        let error = Client::new(ClientOptions {
            app_id: "app".into(),
            ..ClientOptions::default()
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "at least one server node is required");
    }

    #[test]
    fn builder_and_struct_construction_are_equivalent() {
        let from_struct = Client::new(ClientOptions {
            app_id: "app".into(),
            secret: "s".into(),
            nodes: vec!["http://localhost:5000".into()],
            env: "dev".into(),
            cache: CacheOptions {
                enabled: false,
                ..CacheOptions::default()
            },
            ..ClientOptions::default()
        })
        .unwrap();
        let from_builder = Client::builder()
            .app_id("app")
            .secret("s")
            .nodes(["http://localhost:5000"])
            .env("dev")
            .cache(CacheOptions {
                enabled: false,
                ..CacheOptions::default()
            })
            .build()
            .unwrap();
        assert_eq!(from_struct.options().env, "DEV");
        assert_eq!(from_builder.options().app_id, "app");
        assert_eq!(from_builder.options().nodes, from_struct.options().nodes);
    }
}
