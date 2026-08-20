//! WebSocket URL construction, handshake, and background loops.

use std::sync::{Arc, Weak};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, trace, warn};

use crate::auth::basic_authorization;
use crate::client::Inner;
use crate::error::Error;
use crate::options::ClientOptions;
use crate::protocol::{InboundMessage, action, action_module, classify_inbound};

pub(crate) type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
pub(crate) type WsSink = futures_util::stream::SplitSink<WsStream, Message>;

pub(crate) fn http_to_ws_url(
    node: &str,
    name: Option<&str>,
    tag: Option<&str>,
) -> Result<String, Error> {
    let base = if let Some(rest) = strip_prefix_ignore_ascii_case(node, "https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = strip_prefix_ignore_ascii_case(node, "http://") {
        format!("ws://{rest}")
    } else if let Some(rest) = strip_prefix_ignore_ascii_case(node, "wss://") {
        format!("wss://{rest}")
    } else if let Some(rest) = strip_prefix_ignore_ascii_case(node, "ws://") {
        format!("ws://{rest}")
    } else {
        return Err(Error::InvalidNode(node.to_string()));
    };

    let base = base.trim_end_matches('/');
    let name = urlencoding::encode(name.unwrap_or_default());
    let tag = urlencoding::encode(tag.unwrap_or_default());
    Ok(format!("{base}/ws?client_name={name}&client_tag={tag}"))
}

pub(crate) fn websocket_request(node: &str, options: &ClientOptions) -> Result<Request<()>, Error> {
    let url = http_to_ws_url(node, options.name.as_deref(), options.tag.as_deref())?;
    let mut request = url.into_client_request().map_err(Error::websocket)?;
    let headers = request.headers_mut();
    insert_header(headers, "appid", &urlencoding::encode(&options.app_id))?;
    insert_header(headers, "env", &options.env)?;
    insert_header(
        headers,
        "authorization",
        &basic_authorization(&options.app_id, &options.secret),
    )?;
    insert_header(headers, "client-v", env!("CARGO_PKG_VERSION"))?;
    Ok(request)
}

pub(crate) async fn connect(node: &str, options: &ClientOptions) -> Result<WsStream, Error> {
    let request = websocket_request(node, options)?;
    let (stream, _) = connect_async(request).await.map_err(Error::websocket)?;
    Ok(stream)
}

pub(crate) async fn send_text(inner: &Inner, text: &str) -> Result<(), Error> {
    let mut guard = inner.writer.lock().await;
    let Some(writer) = guard.as_mut() else {
        return Ok(());
    };
    writer
        .send(Message::Text(text.into()))
        .await
        .map_err(Error::websocket)?;
    Ok(())
}

pub(crate) async fn close_writer(inner: &Inner) {
    if let Some(mut writer) = inner.writer.lock().await.take() {
        if let Err(error) = writer.close().await {
            debug!(?error, "failed to close websocket writer");
        }
    }
}

pub(crate) fn spawn_reader(
    inner: Weak<Inner>,
    session: CancellationToken,
    mut reader: futures_util::stream::SplitStream<WsStream>,
) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = session.cancelled() => break,
                message = reader.next() => {
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            let Some(inner) = inner.upgrade() else { break };
                            inner.handle_inbound(text.as_ref()).await;
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            debug!("websocket closed by peer");
                            break;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(error)) => {
                            warn!(?error, "websocket receive failed");
                            break;
                        }
                    }
                }
            }
        }

        if let Some(inner) = inner.upgrade() {
            inner.writer.lock().await.take();
        }
    });
}

pub(crate) fn spawn_background_loops(inner: &Arc<Inner>) {
    if inner
        .loops_started
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return;
    }

    spawn_heartbeat(Arc::downgrade(inner));
    spawn_reconnect(Arc::downgrade(inner));
}

fn spawn_heartbeat(inner: Weak<Inner>) {
    tokio::spawn(async move {
        let Some(strong) = inner.upgrade() else {
            return;
        };
        let interval = strong.options.heartbeat_interval;
        let cancel = strong.cancel.clone();
        drop(strong);

        loop {
            tokio::select! {
                () = cancel.cancelled() => break,
                () = tokio::time::sleep(interval) => {
                    let Some(inner) = inner.upgrade() else { break };
                    if let Err(error) = send_text(&inner, "ping").await {
                        trace!(?error, "websocket ping failed");
                    } else if inner.writer.lock().await.is_some() {
                        trace!("client send ping to server by websocket");
                    }
                }
            }
        }
    });
}

fn spawn_reconnect(inner: Weak<Inner>) {
    tokio::spawn(async move {
        let Some(strong) = inner.upgrade() else {
            return;
        };
        let interval = strong.options.reconnect_interval;
        let cancel = strong.cancel.clone();
        drop(strong);

        loop {
            tokio::select! {
                () = cancel.cancelled() => break,
                () = tokio::time::sleep(interval) => {
                    let Some(inner) = inner.upgrade() else { break };
                    if !inner.reconnect_enabled.load(std::sync::atomic::Ordering::SeqCst) {
                        continue;
                    }
                    if inner.writer.lock().await.is_some() {
                        continue;
                    }
                    match inner.connect_websocket().await {
                        Ok(()) => {
                            if let Err(error) = inner.load().await {
                                warn!(?error, "reload after websocket reconnect failed");
                            }
                        }
                        Err(error) => {
                            error!(?error, "client try to connect to server but failed");
                        }
                    }
                }
            }
        }
    });
}

pub(crate) async fn handle_inbound(inner: &Inner, text: &str) {
    trace!(message = text, "client receive message from server");
    match classify_inbound(text) {
        InboundMessage::Drop => {}
        InboundMessage::LegacyVersion(version) => {
            let local = crate::store::data_md5_version(inner.snapshot().data());
            if version != local {
                if let Err(error) = inner.load().await {
                    warn!(?error, "legacy version mismatch reload failed");
                }
            }
        }
        InboundMessage::Action(action) => {
            if !action.module.is_empty() && action.module != action_module::CONFIG_CENTER {
                return;
            }
            match action.action.as_str() {
                action::OFFLINE => {
                    inner.disconnect().await;
                    trace!("client offline because admin console sent offline");
                }
                action::RELOAD => {
                    if let Err(error) = inner.load().await {
                        warn!(?error, "reload action failed");
                    }
                }
                action::PING => {
                    let local = inner.snapshot().version();
                    if action.data != local {
                        trace!(
                            local,
                            remote = action.data.as_str(),
                            "version mismatch, reloading"
                        );
                        if let Err(error) = inner.load().await {
                            warn!(?error, "ping mismatch reload failed");
                        }
                    }
                }
                _ => {}
            }
        }
        InboundMessage::Unknown => {
            trace!(message = text, "ignored websocket message");
        }
    }
}

fn strip_prefix_ignore_ascii_case<'s>(value: &'s str, prefix: &str) -> Option<&'s str> {
    if value.len() >= prefix.len() && value[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&value[prefix.len()..])
    } else {
        None
    }
}

fn insert_header(
    headers: &mut tokio_tungstenite::tungstenite::http::HeaderMap,
    name: &'static str,
    value: &str,
) -> Result<(), Error> {
    let header_name = tokio_tungstenite::tungstenite::http::HeaderName::from_static(name);
    let header_value = tokio_tungstenite::tungstenite::http::HeaderValue::from_str(value)
        .map_err(Error::websocket)?;
    headers.insert(header_name, header_value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::http_to_ws_url;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    #[test]
    fn converts_http_and_https_nodes() {
        assert_eq!(
            http_to_ws_url("http://localhost:5000", Some("n"), Some("t")).unwrap(),
            "ws://localhost:5000/ws?client_name=n&client_tag=t"
        );
        assert_eq!(
            http_to_ws_url("HTTPS://example.com/", None, None).unwrap(),
            "wss://example.com/ws?client_name=&client_tag="
        );
    }

    #[test]
    fn rejects_missing_scheme() {
        assert!(http_to_ws_url("localhost:5000", None, None).is_err());
    }

    #[tokio::test]
    async fn websocket_reload_refetches_http() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Duration;

        use futures_util::{SinkExt, StreamExt};
        use tokio::net::TcpListener;
        use tokio_tungstenite::accept_async;
        use tokio_tungstenite::tungstenite::Message;

        use crate::Client;
        use crate::options::{CacheOptions, ClientOptions};

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_clone = Arc::clone(&hits);
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        tokio::spawn(async move {
            let mut sink_holder = None;
            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        let Ok((stream, _)) = accept else { break };
                        if is_websocket_upgrade(&stream).await {
                            if let Ok(ws) = accept_async(stream).await {
                                let (sink, reader) = ws.split();
                                sink_holder = Some(sink);
                                tokio::spawn(async move {
                                    let mut reader = reader;
                                    while reader.next().await.is_some() {}
                                });
                            }
                        } else {
                            let hits = Arc::clone(&hits_clone);
                            tokio::spawn(async move {
                                serve_http_config(stream, &hits).await;
                            });
                        }
                    }
                    Some(message) = cmd_rx.recv() => {
                        if let Some(sink) = sink_holder.as_mut() {
                            let _ = sink.send(Message::Text(message.into())).await;
                        }
                    }
                }
            }
        });

        let client = Client::new(ClientOptions {
            app_id: "app".into(),
            secret: "secret".into(),
            nodes: vec![format!("http://{addr}")],
            env: "DEV".into(),
            http_timeout: Duration::from_secs(3),
            reconnect_interval: Duration::from_secs(60),
            heartbeat_interval: Duration::from_secs(60),
            cache: CacheOptions {
                enabled: false,
                ..CacheOptions::default()
            },
            ..ClientOptions::default()
        })
        .unwrap();

        client.connect().await.unwrap();
        assert_eq!(client.snapshot().get("n"), Some("1"));

        let mut rx = client.subscribe();
        rx.borrow_and_update();
        cmd_tx.send(r#"{"action":"reload"}"#.into()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), rx.changed())
            .await
            .expect("reload notification")
            .expect("watch channel");
        assert_eq!(client.snapshot().get("n"), Some("2"));
        assert!(hits.load(Ordering::SeqCst) >= 2);
        client.disconnect().await;
    }

    async fn is_websocket_upgrade(stream: &tokio::net::TcpStream) -> bool {
        let mut peek_buf = [0_u8; 1024];
        let Ok(n) = stream.peek(&mut peek_buf).await else {
            return false;
        };
        String::from_utf8_lossy(&peek_buf[..n])
            .to_ascii_lowercase()
            .contains("upgrade: websocket")
    }

    async fn serve_http_config(mut stream: TcpStream, hits: &AtomicUsize) {
        let mut buf = Vec::new();
        loop {
            let mut tmp = [0_u8; 512];
            let Ok(n) = stream.read(&mut tmp).await else {
                return;
            };
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if buf.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let n = hits.fetch_add(1, Ordering::SeqCst);
        let body = if n == 0 {
            r#"[{"key":"n","value":"1","group":""}]"#
        } else {
            r#"[{"key":"n","value":"2","group":""}]"#
        };
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes()).await;
    }
}
