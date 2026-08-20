//! Connect with WebSocket updates and print reload notifications.
//!
//! The `config` crate snapshot is not rebuilt automatically; this example
//! shows how to apply [`agile_config_client::Client::subscribe`] yourself.

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

    client.connect().await?;
    println!("loaded {} item(s)", client.snapshot().items().len());

    let mut rx = client.subscribe();
    while rx.changed().await.is_ok() {
        let snapshot = client.snapshot();
        println!(
            "reload: {} item(s), version={:?}",
            snapshot.items().len(),
            snapshot.publish_time_line_id()
        );
    }
    Ok(())
}
