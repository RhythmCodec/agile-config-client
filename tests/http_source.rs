//! HTTP source integration tests.

use std::time::Duration;

use agile_config_client::{CacheOptions, Client, ClientOptions};
use config::AsyncSource;
use tempfile::TempDir;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn options(nodes: Vec<String>, cache: CacheOptions) -> ClientOptions {
    ClientOptions {
        app_id: "app".into(),
        secret: "secret".into(),
        nodes,
        env: "DEV".into(),
        http_timeout: Duration::from_secs(5),
        cache,
        ..ClientOptions::default()
    }
}

#[tokio::test]
async fn load_pulls_http_and_maps_group_keys() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/config/app/app"))
        .and(query_param("env", "DEV"))
        .and(header("appid", "app"))
        .and(header("Authorization", "Basic YXBwOnNlY3JldA=="))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("publish-time-line-id", "tl-1")
                .set_body_raw(
                    r#"[{"key":"connection","value":"postgres","group":"db"},{"key":"userId","value":"7","group":""}]"#,
                    "application/json",
                ),
        )
        .mount(&server)
        .await;

    let client = Client::new(options(
        vec![server.uri()],
        CacheOptions {
            enabled: false,
            ..CacheOptions::default()
        },
    ))
    .unwrap();
    client.load().await.unwrap();

    let snapshot = client.snapshot();
    assert_eq!(snapshot.get("db:connection"), Some("postgres"));
    assert_eq!(snapshot.get("userId"), Some("7"));
    assert_eq!(snapshot.publish_time_line_id(), Some("tl-1"));
    assert!(!snapshot.from_cache());

    let settings = config::Config::builder()
        .add_async_source(client.source())
        .build()
        .await
        .unwrap();
    assert_eq!(settings.get_string("db.connection").unwrap(), "postgres");
    assert_eq!(settings.get_string("userId").unwrap(), "7");
}

#[tokio::test]
async fn collect_loads_lazily_and_shares_snapshot() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/config/app/app"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"[{"key":"a","value":"b","group":""}]"#,
            "application/json",
        ))
        .mount(&server)
        .await;

    let client = Client::new(options(
        vec![server.uri()],
        CacheOptions {
            enabled: false,
            ..CacheOptions::default()
        },
    ))
    .unwrap();
    assert!(client.snapshot().is_empty());
    let map = client.source().collect().await.unwrap();
    assert_eq!(map.get("a").unwrap().clone().into_string().unwrap(), "b");
    assert_eq!(client.snapshot().get("a"), Some("b"));
}

#[tokio::test]
async fn load_falls_back_to_cache_when_all_nodes_fail() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let dir = TempDir::new().unwrap();
    let cache = CacheOptions {
        enabled: true,
        directory: dir.path().to_path_buf(),
        encrypt: false,
    };
    std::fs::write(
        dir.path().join("app.agileconfig.client.configs.cache"),
        r#"[{"key":"cached","value":"yes","group":""}]"#,
    )
    .unwrap();

    let client = Client::new(options(vec![server.uri()], cache)).unwrap();
    client.load().await.unwrap();
    assert_eq!(client.snapshot().get("cached"), Some("yes"));
    assert!(client.snapshot().from_cache());
}

#[tokio::test]
async fn load_fails_without_cache_when_http_fails() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let client = Client::new(options(
        vec![server.uri()],
        CacheOptions {
            enabled: false,
            ..CacheOptions::default()
        },
    ))
    .unwrap();
    let error = client.load().await.unwrap_err();
    assert!(
        error.to_string().contains("HTTP 503") || error.to_string().contains("failed to load"),
        "{error}"
    );
}

#[tokio::test]
async fn reload_action_pulls_http_again() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/config/app/app"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"[{"key":"n","value":"1","group":""}]"#,
            "application/json",
        ))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/config/app/app"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"[{"key":"n","value":"2","group":""}]"#,
            "application/json",
        ))
        .mount(&server)
        .await;

    let client = Client::new(options(
        vec![server.uri()],
        CacheOptions {
            enabled: false,
            ..CacheOptions::default()
        },
    ))
    .unwrap();
    client.load().await.unwrap();
    assert_eq!(client.snapshot().get("n"), Some("1"));
    client.load().await.unwrap();
    assert_eq!(client.snapshot().get("n"), Some("2"));
}
