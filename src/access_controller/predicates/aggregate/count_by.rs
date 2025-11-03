// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use std::str::FromStr;

const HTTP_HEADER_PREFIX: &str = "http-header::";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CountBy {
    SenderAddress,
    HttpHeader(CountByHttpHeader),
}

impl CountBy {
    pub fn new_http_header(header_name: impl AsRef<str>) -> Self {
        CountBy::HttpHeader(CountByHttpHeader {
            header_name: header_name.as_ref().to_string(),
        })
    }

    pub fn new_sender_address() -> Self {
        CountBy::SenderAddress
    }
}

impl Serialize for CountBy {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            CountBy::SenderAddress => serializer.serialize_str("sender-address"),
            CountBy::HttpHeader(header) => serializer.serialize_str(&header.to_string()),
        }
    }
}

impl<'de> Deserialize<'de> for CountBy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "sender-address" => Ok(CountBy::SenderAddress),
            _ if s.starts_with(HTTP_HEADER_PREFIX) => {
                let header = CountByHttpHeader::from_str(&s).map_err(serde::de::Error::custom)?;
                Ok(CountBy::HttpHeader(header))
            }
            _ => Err(serde::de::Error::custom(format!("Invalid CountBy: {}", s))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountByHttpHeader {
    pub header_name: String,
}

impl Serialize for CountByHttpHeader {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for CountByHttpHeader {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(CountByHttpHeader::from_str(&s).unwrap())
    }
}

// The HttpHeader should be serialized to string like: http-header::<header-name>
impl FromStr for CountByHttpHeader {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts = s.split(HTTP_HEADER_PREFIX).collect::<Vec<&str>>();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!("Invalid HttpHeader: {}", s));
        }
        Ok(CountByHttpHeader {
            header_name: parts[1].to_string(),
        })
    }
}

impl From<&CountByHttpHeader> for String {
    fn from(header: &CountByHttpHeader) -> Self {
        format!("{HTTP_HEADER_PREFIX}{}", header.header_name)
    }
}

impl TryFrom<&String> for CountByHttpHeader {
    type Error = anyhow::Error;

    fn try_from(s: &String) -> Result<Self, Self::Error> {
        CountByHttpHeader::from_str(&s).map_err(|e| anyhow::anyhow!("Invalid HttpHeader: {}", e))
    }
}

impl ToString for CountBy {
    fn to_string(&self) -> String {
        match self {
            CountBy::SenderAddress => "sender-address".to_string(),
            CountBy::HttpHeader(header) => header.to_string(),
        }
    }
}

impl ToString for CountByHttpHeader {
    fn to_string(&self) -> String {
        self.into()
    }
}

#[cfg(test)]
mod test {
    use super::{CountBy, CountByHttpHeader};
    #[test]
    fn test_serde_count_by_sender_address() {
        let count_by = CountBy::SenderAddress;
        let json = serde_json::to_string(&count_by).unwrap();
        assert_eq!(json, r#""sender-address""#);

        let count_by: CountBy = serde_json::from_str(&json).unwrap();
        assert_eq!(count_by, CountBy::SenderAddress);
    }

    #[test]
    fn test_serde_count_by_http_header() {
        let count_by = CountBy::HttpHeader(CountByHttpHeader {
            header_name: "X-Forwarded-For".to_string(),
        });
        let json = serde_json::to_string(&count_by).unwrap();
        assert_eq!(json, r#""http-header::X-Forwarded-For""#);

        let limit_by: CountBy = serde_json::from_str(&json).unwrap();
        assert_eq!(
            limit_by,
            CountBy::HttpHeader(CountByHttpHeader {
                header_name: "X-Forwarded-For".to_string(),
            })
        );
    }
}
