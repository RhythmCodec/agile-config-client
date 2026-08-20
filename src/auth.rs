//! HTTP Basic authentication helpers compatible with the C# client.

pub(crate) fn basic_authorization(app_id: &str, secret: &str) -> String {
    use base64::Engine as _;
    let token =
        base64::engine::general_purpose::STANDARD.encode(format!("{app_id}:{secret}").as_bytes());
    format!("Basic {token}")
}

#[cfg(test)]
mod tests {
    use super::basic_authorization;

    #[test]
    fn basic_authorization_matches_utf8_base64() {
        // base64("app:secret") = YXBwOnNlY3JldA==
        assert_eq!(
            basic_authorization("app", "secret"),
            "Basic YXBwOnNlY3JldA=="
        );
    }
}
