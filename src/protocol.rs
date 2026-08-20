//! Wire protocol types and constants used by `AgileConfig` nodes.

use serde::{Deserialize, Deserializer};

/// Action names sent over the configuration WebSocket.
pub mod action {
    /// Add a configuration item (legacy).
    #[allow(dead_code)]
    pub const ADD: &str = "add";
    /// Remove a configuration item (legacy).
    #[allow(dead_code)]
    pub const REMOVE: &str = "remove";
    /// Update a configuration item (legacy).
    #[allow(dead_code)]
    pub const UPDATE: &str = "update";
    /// Instruct the client to disconnect and stop reconnecting.
    pub const OFFLINE: &str = "offline";
    /// Instruct the client to pull configuration again.
    pub const RELOAD: &str = "reload";
    /// Compare configuration versions.
    pub const PING: &str = "ping";
}

/// Action modules that distinguish configuration vs registration messages.
pub mod action_module {
    /// Service registration center.
    #[allow(dead_code)]
    pub const REGISTER_CENTER: &str = "r";
    /// Configuration center.
    pub const CONFIG_CENTER: &str = "c";
}

/// HTTP response header carrying the published configuration version.
pub const HEADER_KEY_PUBLISH_TIME_LINE_ID: &str = "publish-time-line-id";

/// One published configuration item as returned by the HTTP API.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct ConfigItem {
    /// Item key within its group.
    #[serde(default, alias = "Key", deserialize_with = "null_to_default")]
    pub key: String,
    /// Item value.
    #[serde(default, alias = "Value", deserialize_with = "null_to_default")]
    pub value: String,
    /// Optional group. Combined with [`Self::key`] as `group:key`.
    ///
    /// The server may send JSON `null` for ungrouped items; that becomes empty.
    #[serde(default, alias = "Group", deserialize_with = "null_to_default")]
    pub group: String,
}

impl ConfigItem {
    /// Returns the C#-compatible dictionary key: `group:key` or `key`.
    #[must_use]
    pub fn composed_key(&self) -> String {
        if self.group.is_empty() {
            self.key.clone()
        } else {
            format!("{}:{}", self.group, self.key)
        }
    }
}

/// WebSocket action payload.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct ActionMessage {
    #[serde(default, alias = "Module", deserialize_with = "null_to_default")]
    pub module: String,
    #[serde(default, alias = "Action", deserialize_with = "null_to_default")]
    pub action: String,
    #[serde(default, alias = "Data", deserialize_with = "null_to_default")]
    pub data: String,
}

/// Classified inbound WebSocket text frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InboundMessage {
    Drop,
    LegacyVersion(String),
    Action(ActionMessage),
    Unknown,
}

/// Treats JSON `null` as `T::default()` (empty string for [`String`]).
fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// Classifies a raw WebSocket text payload.
pub(crate) fn classify_inbound(text: &str) -> InboundMessage {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed == "0" {
        return InboundMessage::Drop;
    }
    if let Some(version) = trimmed.strip_prefix("V:") {
        return InboundMessage::LegacyVersion(version.to_string());
    }
    match serde_json::from_str::<ActionMessage>(trimmed) {
        Ok(message) => InboundMessage::Action(message),
        Err(_) => InboundMessage::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActionMessage, ConfigItem, InboundMessage, action, action_module, classify_inbound,
    };

    #[test]
    fn protocol_constants_match_csharp_client() {
        assert_eq!(action::ADD, "add");
        assert_eq!(action::REMOVE, "remove");
        assert_eq!(action::UPDATE, "update");
        assert_eq!(action::OFFLINE, "offline");
        assert_eq!(action::RELOAD, "reload");
        assert_eq!(action::PING, "ping");
        assert_eq!(action_module::REGISTER_CENTER, "r");
        assert_eq!(action_module::CONFIG_CENTER, "c");
    }

    #[test]
    fn composed_key_includes_group_when_present() {
        let item = ConfigItem {
            key: "connection".into(),
            value: "x".into(),
            group: "db".into(),
        };
        assert_eq!(item.composed_key(), "db:connection");
    }

    #[test]
    fn composed_key_omits_empty_group() {
        let item = ConfigItem {
            key: "userId".into(),
            value: "1".into(),
            group: String::new(),
        };
        assert_eq!(item.composed_key(), "userId");
    }

    #[test]
    fn classify_drops_legacy_heartbeat() {
        assert_eq!(classify_inbound("0"), InboundMessage::Drop);
        assert_eq!(classify_inbound(""), InboundMessage::Drop);
        assert_eq!(classify_inbound("   "), InboundMessage::Drop);
    }

    #[test]
    fn classify_legacy_version_prefix() {
        assert_eq!(
            classify_inbound("V:ABC123"),
            InboundMessage::LegacyVersion("ABC123".into())
        );
    }

    #[test]
    fn classify_action_is_case_insensitive_on_field_names() {
        let inbound = classify_inbound(r#"{"Module":"c","Action":"reload","Data":""}"#);
        assert_eq!(
            inbound,
            InboundMessage::Action(ActionMessage {
                module: "c".into(),
                action: "reload".into(),
                data: String::new(),
            })
        );

        let inbound = classify_inbound(r#"{"action":"ping","data":"v1"}"#);
        assert_eq!(
            inbound,
            InboundMessage::Action(ActionMessage {
                module: String::new(),
                action: "ping".into(),
                data: "v1".into(),
            })
        );
    }

    #[test]
    fn parses_config_item_aliases() {
        let item: ConfigItem =
            serde_json::from_str(r#"{"Key":"a","Value":"b","Group":"g"}"#).unwrap();
        assert_eq!(
            item,
            ConfigItem {
                key: "a".into(),
                value: "b".into(),
                group: "g".into(),
            }
        );
    }

    #[test]
    fn parses_server_payload_with_null_group_and_extra_fields() {
        let json = r#"[
            {"id":"e5fc1e1737ad48d0aa4c34a47d31f595","group":null,"key":"pub_key","value":"xxx114514","status":0,"onlineStatus":0,"editStatus":0,"description":null,"appId":null},
            {"id":"1cd1166da804442b8855965af559fac9","group":null,"key":"base_url","value":"example.com","status":0,"onlineStatus":0,"editStatus":0,"description":null,"appId":null},
            {"id":"b17c49653c0542de9df9c76eeeeb2f73","group":"test_group","key":"test","value":"ok","status":0,"onlineStatus":0,"editStatus":0,"description":null,"appId":null}
        ]"#;
        let items: Vec<ConfigItem> = serde_json::from_str(json).unwrap();
        assert_eq!(
            items,
            vec![
                ConfigItem {
                    key: "pub_key".into(),
                    value: "xxx114514".into(),
                    group: String::new(),
                },
                ConfigItem {
                    key: "base_url".into(),
                    value: "example.com".into(),
                    group: String::new(),
                },
                ConfigItem {
                    key: "test".into(),
                    value: "ok".into(),
                    group: "test_group".into(),
                },
            ]
        );
        assert_eq!(items[0].composed_key(), "pub_key");
        assert_eq!(items[2].composed_key(), "test_group:test");
    }
}
