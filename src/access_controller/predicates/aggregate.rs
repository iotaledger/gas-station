use std::{str::FromStr, time::Duration};

use serde::{Deserialize, Serialize};

use super::ValueNumber;

const HTTP_HEADER_PREFIX: &str = "http-header::";

/// ValueAggregate is a struct that represents an aggregate value with a specified window and limit.
/// It must use persistent storage [`Tracker`] to store the aggregate value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ValueAggregate {
    #[serde(with = "serde_duration")]
    pub window: Duration,
    pub value: ValueNumber<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub count_by: Vec<CountBy>,
}

impl ValueAggregate {
    pub fn new(window: Duration, limit: ValueNumber<u64>) -> Self {
        ValueAggregate {
            window,
            value: limit,
            count_by: vec![],
        }
    }

    pub fn with_count_by(mut self, group_by: Vec<CountBy>) -> Self {
        self.count_by = group_by;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
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

mod serde_duration {
    use serde::Deserialize;

    fn parse_duration(s: &str) -> std::time::Duration {
        humantime::parse_duration(s).unwrap_or_else(|_| panic!("Failed to parse duration: {}", s))
    }

    pub fn serialize<S>(value: &std::time::Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = humantime::format_duration(*value).to_string();
        serializer.serialize_str(&s)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<std::time::Duration, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(parse_duration(&s))
    }
}

#[cfg(test)]
mod test {
    use super::{CountBy, CountByHttpHeader};
    use crate::access_controller::predicates::{ValueAggregate, ValueNumber};

    #[test]
    fn test_serde_limit_by() {
        let limit_by = CountBy::HttpHeader(CountByHttpHeader {
            header_name: "X-Forwarded-For".to_string(),
        });
        let json = serde_json::to_string(&limit_by).unwrap();
        assert_eq!(json, r#""http-header::X-Forwarded-For""#);

        let limit_by: CountBy = serde_json::from_str(&json).unwrap();
        assert_eq!(
            limit_by,
            CountBy::HttpHeader(CountByHttpHeader {
                header_name: "X-Forwarded-For".to_string(),
            })
        );
    }

    #[test]
    fn test_deserialize_value_aggregate() {
        let json = r#"{"window":"1h 30 min","value": ">100"}"#;
        let value_aggregate: ValueAggregate = serde_json::from_str(json).unwrap();

        assert_eq!(value_aggregate.window.as_secs(), 5400);
        assert!(matches!(
            value_aggregate.value,
            ValueNumber::GreaterThan(100),
        ));
    }

    #[test]
    fn test_serialize_value_aggregate() {
        let value_aggregate = ValueAggregate::new(
            std::time::Duration::new(5400, 0),
            ValueNumber::GreaterThan(100),
        );
        let json = serde_json::to_string(&value_aggregate).unwrap();
        assert_eq!(json, r#"{"window":"1h 30m","value":">100"}"#);
    }
}
