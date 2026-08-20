//! `config` crate [`AsyncSource`] adapter.

use std::fmt::{Debug, Formatter};
use std::sync::Arc;

use async_trait::async_trait;
use config::{AsyncSource, ConfigError, Map, Value};

use crate::client::Inner;

/// Read-only configuration source backed by a [`crate::Client`].
///
/// `collect` returns the current snapshot. If the client has not loaded yet,
/// it performs an HTTP pull (with cache fallback). It never starts the
/// WebSocket session.
#[derive(Clone)]
pub struct Source {
    inner: Arc<Inner>,
}

impl Source {
    pub(crate) fn new(inner: Arc<Inner>) -> Self {
        Self { inner }
    }
}

impl Debug for Source {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Source")
            .field("app_id", &self.inner.options.app_id)
            .finish()
    }
}

#[async_trait]
impl AsyncSource for Source {
    async fn collect(&self) -> Result<Map<String, Value>, ConfigError> {
        self.inner
            .ensure_loaded()
            .await
            .map_err(|error| ConfigError::Foreign(Box::new(error)))?;
        Ok(self.inner.snapshot().to_config_map())
    }
}
