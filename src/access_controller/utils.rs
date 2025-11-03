// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use axum::http::HeaderMap;

pub fn header_map_to_btree_map(headers: &HeaderMap) -> BTreeMap<String, Vec<String>> {
    let mut header_btree_map = BTreeMap::new();
    for (k, v) in headers.clone().iter() {
        let k = k.to_string();
        let v = String::from_utf8_lossy(v.as_bytes()).into_owned();
        header_btree_map.entry(k).or_insert_with(Vec::new).push(v);
    }
    header_btree_map
}

#[cfg(test)]
mod test {
    use axum::http::{HeaderName, HeaderValue};
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_header_map_to_btree_map() {
        let headers = HeaderMap::from_iter([(
            HeaderName::from_str("X-Account-Id").unwrap(),
            HeaderValue::from_str("123").unwrap(),
        )]);
        let btree_map = header_map_to_btree_map(&headers);
        assert_eq!(
            btree_map,
            BTreeMap::from_iter([("x-account-id".to_string(), vec!["123".to_string()])])
        );
    }

    #[test]
    fn test_header_map_to_btree_map_with_multiple_values() {
        let headers = HeaderMap::from_iter([
            (
                HeaderName::from_str("X-Account-Id").unwrap(),
                HeaderValue::from_str("123").unwrap(),
            ),
            (
                HeaderName::from_str("X-Account-Id").unwrap(),
                HeaderValue::from_str("456").unwrap(),
            ),
        ]);
        let btree_map = header_map_to_btree_map(&headers);
        assert_eq!(
            btree_map,
            BTreeMap::from_iter([(
                "x-account-id".to_string(),
                vec!["123".to_string(), "456".to_string()]
            )])
        );
    }
}
