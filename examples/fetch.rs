//! One-shot configuration pull using [`agile_config_client::Source`].
//!
//! Set `AGILE_CONFIG_APP_ID`, `AGILE_CONFIG_SECRET`, `AGILE_CONFIG_NODES`,
//! and optionally `AGILE_CONFIG_ENV`.

use agile_config_client::{Client, ClientOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new(ClientOptions {
        app_id: std::env::var("AGILE_CONFIG_APP_ID")?,
        secret: std::env::var("AGILE_CONFIG_SECRET").unwrap_or_default(),
        nodes: std::env::var("AGILE_CONFIG_NODES")?
            .split(',')
            .map(|node| node.trim().to_string())
            .filter(|node| !node.is_empty())
            .collect(),
        env: std::env::var("AGILE_CONFIG_ENV").unwrap_or_else(|_| "DEV".into()),
        ..ClientOptions::default()
    })?;

    let settings = config::Config::builder()
        .add_async_source(client.source())
        .build()
        .await?;

    println!("{settings:?}");
    Ok(())
}
