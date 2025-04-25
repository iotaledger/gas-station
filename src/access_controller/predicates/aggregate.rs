use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::ValueNumber;

/// ValueAggregate is a struct that represents an aggregate value with a specified window and limit.
/// It must use persistent storage [`Tracker`] to store the aggregate value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ValueAggregate {
    #[serde(with = "serde_duration")]
    pub window: Duration,
    pub value: ValueNumber<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub count_by: Vec<LimitBy>,
}

impl ValueAggregate {
    pub fn new(window: Duration, limit: ValueNumber<u64>) -> Self {
        ValueAggregate {
            window,
            value: limit,
            count_by: vec![],
        }
    }

    pub fn with_count_by(mut self, group_by: Vec<LimitBy>) -> Self {
        self.count_by = group_by;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LimitBy {
    SenderAddress,
}

impl ToString for LimitBy {
    fn to_string(&self) -> String {
        match self {
            LimitBy::SenderAddress => "sender-address".to_string(),
        }
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
    use crate::access_controller::predicates::{ValueAggregate, ValueNumber};

    #[test]
    fn test_deserialize_value_aggregate() {
        let json = r#"{"window":"1h 30 min","limit": ">100"}"#;
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
        assert_eq!(json, r#"{"window":"1h 30m","limit":">100"}"#);
    }
}
