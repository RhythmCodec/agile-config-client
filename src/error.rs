//! Typed errors returned by the `AgileConfig` client.

/// Failures that can occur while configuring, loading, or connecting.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// `app_id` was empty or whitespace.
    #[error("app_id must not be empty")]
    EmptyAppId,
    /// No usable server node URLs were provided.
    #[error("at least one server node is required")]
    EmptyNodes,
    /// A node URL could not be converted to a WebSocket endpoint.
    #[error("invalid server node URL: {0}")]
    InvalidNode(String),
    /// Every node failed and no local cache was available.
    #[error("failed to load configuration from all nodes")]
    LoadFailed,
    /// The server returned a non-success HTTP status.
    #[error("HTTP {status} from {url}")]
    HttpStatus {
        /// Request URL.
        url: String,
        /// HTTP status code.
        status: u16,
    },
    /// Underlying HTTP client error.
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    /// WebSocket handshake or I/O failed.
    #[error("websocket error: {0}")]
    WebSocket(String),
    /// The configuration payload could not be decoded.
    #[error("failed to parse configuration payload: {0}")]
    Protocol(#[from] serde_json::Error),
    /// Reading or writing the local cache file failed.
    #[error("cache error: {0}")]
    Cache(String),
    /// [`CacheOptions::encrypt`](crate::CacheOptions::encrypt) is `true`, but
    /// the crate was built without the `cache-encrypt` feature.
    #[error("cache encryption requires the `cache-encrypt` cargo feature")]
    CacheEncryptDisabled,
}

impl Error {
    pub(crate) fn websocket<E: std::fmt::Display>(error: E) -> Self {
        Self::WebSocket(error.to_string())
    }

    pub(crate) fn cache<E: std::fmt::Display>(error: E) -> Self {
        Self::Cache(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn empty_app_id_displays_clear_message() {
        assert_eq!(Error::EmptyAppId.to_string(), "app_id must not be empty");
    }
}
