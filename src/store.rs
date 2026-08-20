//! In-memory configuration snapshot and conversion to `config::Map`.

use std::collections::HashMap;
use std::sync::Arc;

use config::{Map, Value};
use md5::{Digest, Md5};

use crate::protocol::ConfigItem;

/// Point-in-time view of configuration pulled from `AgileConfig`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigSnapshot {
    data: HashMap<String, String>,
    items: Vec<ConfigItem>,
    publish_time_line_id: Option<String>,
    from_cache: bool,
}

impl ConfigSnapshot {
    pub(crate) fn from_items(
        items: Vec<ConfigItem>,
        publish_time_line_id: Option<String>,
        from_cache: bool,
    ) -> Self {
        let mut data = HashMap::with_capacity(items.len());
        for item in &items {
            let key = item.composed_key();
            data.entry(key).or_insert_with(|| item.value.clone());
        }
        Self {
            data,
            items,
            publish_time_line_id: publish_time_line_id.filter(|id| !id.is_empty()),
            from_cache,
        }
    }

    /// Looks up a value by the C#-compatible key (`group:key` or `key`).
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.data.get(key).map(String::as_str)
    }

    /// Flat dictionary using C#-compatible keys.
    #[must_use]
    pub fn data(&self) -> &HashMap<String, String> {
        &self.data
    }

    /// Original items returned by the server.
    #[must_use]
    pub fn items(&self) -> &[ConfigItem] {
        &self.items
    }

    /// Published version header, when the server provided one.
    #[must_use]
    pub fn publish_time_line_id(&self) -> Option<&str> {
        self.publish_time_line_id.as_deref()
    }

    /// Whether this snapshot was restored from the local cache.
    #[must_use]
    pub fn from_cache(&self) -> bool {
        self.from_cache
    }

    /// Returns `true` when no items are present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Converts `group:key` entries into a nested [`config::Map`] using `.` as the path separator.
    #[must_use]
    pub fn to_config_map(&self) -> Map<String, Value> {
        let mut root = Map::new();
        for (key, value) in &self.data {
            let dotted = key.replace(':', ".");
            insert_dotted(&mut root, &dotted, value.clone());
        }
        root
    }

    pub(crate) fn version(&self) -> String {
        if let Some(id) = &self.publish_time_line_id {
            id.clone()
        } else {
            data_md5_version(&self.data)
        }
    }
}

pub(crate) fn data_md5_version(data: &HashMap<String, String>) -> String {
    let mut keys: Vec<&str> = data.keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut values: Vec<&str> = data.values().map(String::as_str).collect();
    values.sort_unstable();
    let txt = format!("{}&{}", keys.join("&"), values.join("&"));
    md5_hex_ascii(&txt)
}

fn md5_hex_ascii(text: &str) -> String {
    let digest = Md5::digest(text.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02X}");
    }
    out
}

fn insert_dotted(map: &mut Map<String, Value>, path: &str, value: String) {
    let parts: Vec<&str> = path.split('.').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        return;
    }
    insert_parts(map, &parts, value);
}

fn insert_parts(map: &mut Map<String, Value>, parts: &[&str], value: String) {
    let Some((first, rest)) = parts.split_first() else {
        return;
    };
    if rest.is_empty() {
        map.insert((*first).to_string(), Value::from(value));
        return;
    }

    let mut nested = match map.get_mut(*first) {
        Some(existing) => existing.clone().into_table().unwrap_or_default(),
        None => Map::new(),
    };
    insert_parts(&mut nested, rest, value);
    map.insert((*first).to_string(), Value::from(nested));
}

pub(crate) fn empty_snapshot() -> Arc<ConfigSnapshot> {
    Arc::new(ConfigSnapshot::default())
}

#[cfg(test)]
mod tests {
    use super::{ConfigSnapshot, data_md5_version};
    use crate::protocol::ConfigItem;
    use std::collections::HashMap;

    #[test]
    fn composed_keys_first_write_wins() {
        let snapshot = ConfigSnapshot::from_items(
            vec![
                ConfigItem {
                    key: "a".into(),
                    value: "first".into(),
                    group: String::new(),
                },
                ConfigItem {
                    key: "a".into(),
                    value: "second".into(),
                    group: String::new(),
                },
            ],
            None,
            false,
        );
        assert_eq!(snapshot.get("a"), Some("first"));
    }

    #[test]
    fn to_config_map_nests_group_and_colon_keys() {
        let snapshot = ConfigSnapshot::from_items(
            vec![
                ConfigItem {
                    key: "connection".into(),
                    value: "postgres".into(),
                    group: "db".into(),
                },
                ConfigItem {
                    key: "userId".into(),
                    value: "1".into(),
                    group: String::new(),
                },
            ],
            None,
            false,
        );
        let map = snapshot.to_config_map();
        let db = map.get("db").unwrap().clone().into_table().unwrap();
        assert_eq!(
            db.get("connection").unwrap().clone().into_string().unwrap(),
            "postgres"
        );
        assert_eq!(
            map.get("userId").unwrap().clone().into_string().unwrap(),
            "1"
        );
    }

    #[test]
    fn md5_version_matches_empty_dictionary_rule() {
        let data = HashMap::new();
        // C# joins empty key/value lists then inserts `&` between them.
        assert_eq!(data_md5_version(&data), md5_upper("&"));
    }

    #[test]
    fn md5_version_uses_ordinal_sort_independent_of_pairing() {
        let mut data = HashMap::new();
        data.insert("a".into(), "2".into());
        data.insert("b".into(), "1".into());
        assert_eq!(data_md5_version(&data), md5_upper("a&b&1&2"));
    }

    fn md5_upper(text: &str) -> String {
        super::md5_hex_ascii(text)
    }
}
