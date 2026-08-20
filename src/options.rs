//! Client construction options and builder.

use std::path::PathBuf;
use std::time::Duration;

use crate::client::Client;
use crate::error::Error;

/// Connection and cache settings for [`Client`].
///
/// Fields are public so callers can construct this struct directly. Invalid
/// combinations (empty `app_id` or `nodes`) are rejected by [`Client::new`].
///
/// # Examples
///
/// ```
/// use agile_config_client::ClientOptions;
///
/// let options = ClientOptions {
///     app_id: "app".into(),
///     secret: "secret".into(),
///     nodes: vec!["http://localhost:5000".into()],
///     env: "DEV".into(),
///     ..ClientOptions::default()
/// };
/// assert_eq!(options.app_id, "app");
/// ```
#[derive(Clone, Debug)]
pub struct ClientOptions {
    /// Application id configured in the `AgileConfig` console.
    pub app_id: String,
    /// Application secret. May be empty when the app has no secret.
    pub secret: String,
    /// Server node base URLs (`http://` or `https://`).
    ///
    /// Entries may be comma-separated; they are split and trimmed in [`Client::new`].
    pub nodes: Vec<String>,
    /// Target environment. Empty lets the server pick its default.
    pub env: String,
    /// Optional display name shown in the admin console.
    pub name: Option<String>,
    /// Optional tag shown in the admin console.
    pub tag: Option<String>,
    /// Timeout for HTTP configuration pulls.
    pub http_timeout: Duration,
    /// Delay between WebSocket reconnect attempts.
    pub reconnect_interval: Duration,
    /// Interval for sending WebSocket `ping` text frames.
    pub heartbeat_interval: Duration,
    /// Local file cache settings.
    pub cache: CacheOptions,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            app_id: String::new(),
            secret: String::new(),
            nodes: Vec::new(),
            env: String::new(),
            name: None,
            tag: None,
            http_timeout: Duration::from_secs(100),
            reconnect_interval: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(30),
            cache: CacheOptions::default(),
        }
    }
}

impl ClientOptions {
    /// Starts a builder that fills a [`ClientOptions`] value.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    pub(crate) fn normalized(mut self) -> Result<Self, Error> {
        self.app_id = self.app_id.trim().to_string();
        if self.app_id.is_empty() {
            return Err(Error::EmptyAppId);
        }

        self.nodes = normalize_nodes(&self.nodes);
        if self.nodes.is_empty() {
            return Err(Error::EmptyNodes);
        }

        self.env = self.env.trim().to_ascii_uppercase();
        self.secret = self.secret.trim().to_string();
        self.name = trim_optional(self.name);
        self.tag = trim_optional(self.tag);

        if self.http_timeout.is_zero() {
            self.http_timeout = Duration::from_secs(30);
        }
        if self.reconnect_interval.is_zero() {
            self.reconnect_interval = Duration::from_secs(5);
        }
        if self.heartbeat_interval.is_zero() {
            self.heartbeat_interval = Duration::from_secs(30);
        }

        #[cfg(not(feature = "cache-encrypt"))]
        if self.cache.encrypt {
            return Err(Error::CacheEncryptDisabled);
        }

        Ok(self)
    }
}

/// Local cache of the last successfully pulled configuration JSON.
#[derive(Clone, Debug)]
pub struct CacheOptions {
    /// When `true`, successful pulls are written to disk and used if HTTP fails.
    pub enabled: bool,
    /// Directory for the cache file. Empty means the process working directory.
    pub directory: PathBuf,
    /// When `true`, cache contents are AES-encrypted with the application secret.
    ///
    /// Requires the `cache-encrypt` crate feature. [`Client::new`] returns
    /// [`Error::CacheEncryptDisabled`] if this is set without that feature.
    pub encrypt: bool,
}

impl Default for CacheOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            directory: PathBuf::new(),
            encrypt: false,
        }
    }
}

/// Fluent builder for [`Client`] / [`ClientOptions`].
#[derive(Clone, Debug, Default)]
#[must_use]
pub struct ClientBuilder {
    options: ClientOptions,
}

impl ClientBuilder {
    /// Sets the application id.
    pub fn app_id(mut self, app_id: impl Into<String>) -> Self {
        self.options.app_id = app_id.into();
        self
    }

    /// Sets the application secret.
    pub fn secret(mut self, secret: impl Into<String>) -> Self {
        self.options.secret = secret.into();
        self
    }

    /// Sets the server node URLs.
    pub fn nodes<I, S>(mut self, nodes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.options.nodes = nodes.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the environment name.
    pub fn env(mut self, env: impl Into<String>) -> Self {
        self.options.env = env.into();
        self
    }

    /// Sets the client display name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.options.name = Some(name.into());
        self
    }

    /// Sets the client tag.
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.options.tag = Some(tag.into());
        self
    }

    /// Sets the HTTP timeout.
    pub fn http_timeout(mut self, timeout: Duration) -> Self {
        self.options.http_timeout = timeout;
        self
    }

    /// Sets the WebSocket reconnect interval.
    pub fn reconnect_interval(mut self, interval: Duration) -> Self {
        self.options.reconnect_interval = interval;
        self
    }

    /// Sets the WebSocket ping interval.
    pub fn heartbeat_interval(mut self, interval: Duration) -> Self {
        self.options.heartbeat_interval = interval;
        self
    }

    /// Replaces cache settings.
    pub fn cache(mut self, cache: CacheOptions) -> Self {
        self.options.cache = cache;
        self
    }

    /// Builds a [`Client`], validating required fields.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmptyAppId`] or [`Error::EmptyNodes`] when required
    /// fields are missing.
    pub fn build(self) -> Result<Client, Error> {
        Client::new(self.options)
    }

    /// Builds a validated [`ClientOptions`] value without creating a client.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmptyAppId`] or [`Error::EmptyNodes`] when required
    /// fields are missing.
    pub fn build_options(self) -> Result<ClientOptions, Error> {
        self.options.normalized()
    }
}

pub(crate) fn normalize_nodes(nodes: &[String]) -> Vec<String> {
    nodes
        .iter()
        .flat_map(|node| node.split(','))
        .map(str::trim)
        .filter(|node| !node.is_empty())
        .map(|node| node.trim_end_matches('/').to_string())
        .collect()
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{ClientOptions, normalize_nodes};

    #[test]
    fn normalize_nodes_splits_commas_and_strips_slashes() {
        let nodes = vec![
            " http://localhost:5000/ ".into(),
            "http://n2:1,http://n3:2/".into(),
        ];
        assert_eq!(
            normalize_nodes(&nodes),
            vec![
                "http://localhost:5000".to_string(),
                "http://n2:1".to_string(),
                "http://n3:2".to_string(),
            ]
        );
    }

    #[test]
    fn normalized_rejects_empty_app_id() {
        let error = ClientOptions {
            nodes: vec!["http://localhost:5000".into()],
            ..ClientOptions::default()
        }
        .normalized()
        .unwrap_err();
        assert_eq!(error.to_string(), "app_id must not be empty");
    }

    #[test]
    fn normalized_uppercases_env() {
        let options = ClientOptions {
            app_id: "app".into(),
            nodes: vec!["http://localhost:5000".into()],
            env: " dev ".into(),
            ..ClientOptions::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(options.env, "DEV");
    }

    #[cfg(not(feature = "cache-encrypt"))]
    #[test]
    fn normalized_rejects_encrypt_without_feature() {
        use super::CacheOptions;
        use crate::Error;

        let error = ClientOptions {
            app_id: "app".into(),
            nodes: vec!["http://localhost:5000".into()],
            cache: CacheOptions {
                encrypt: true,
                ..CacheOptions::default()
            },
            ..ClientOptions::default()
        }
        .normalized()
        .unwrap_err();
        assert!(matches!(error, Error::CacheEncryptDisabled));
    }

    #[cfg(feature = "cache-encrypt")]
    #[test]
    fn normalized_allows_encrypt_with_feature() {
        use super::CacheOptions;

        ClientOptions {
            app_id: "app".into(),
            nodes: vec!["http://localhost:5000".into()],
            cache: CacheOptions {
                encrypt: true,
                ..CacheOptions::default()
            },
            ..ClientOptions::default()
        }
        .normalized()
        .unwrap();
    }
}
