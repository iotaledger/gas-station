// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

pub const OP_GE: &str = ">=";
pub const OP_LE: &str = "<=";
pub const OP_EQ: &str = "=";
pub const OP_NE: &str = "!=";
pub const OP_GT: &str = ">";
pub const OP_LT: &str = "<";

// The ValueNumber represents the number value in the rule. It can represent a single number or a range of number
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueNumber {
    GreaterThan(u64),
    LessThan(u64),
    Equal(u64),
    NotEqual(u64),
    GreaterThanOrEqual(u64),
    LessThanOrEqual(u64),
}

impl From<u64> for ValueNumber {
    fn from(value: u64) -> Self {
        ValueNumber::Equal(value)
    }
}

impl ValueNumber {
    /// Check if the value matches the number.
    pub fn matches(&self, value: u64) -> bool {
        match self {
            ValueNumber::GreaterThan(number) => value > *number,
            ValueNumber::LessThan(number) => value < *number,
            ValueNumber::Equal(number) => value == *number,
            ValueNumber::NotEqual(number) => value != *number,
            ValueNumber::GreaterThanOrEqual(number) => value >= *number,
            ValueNumber::LessThanOrEqual(number) => value <= *number,
        }
    }
}

impl Serialize for ValueNumber {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ValueNumber::GreaterThan(number) => {
                serializer.serialize_str(&format!("{}{}", OP_GT, number))
            }
            ValueNumber::LessThan(number) => {
                serializer.serialize_str(&format!("{}{}", OP_LT, number))
            }
            ValueNumber::Equal(number) => serializer.serialize_str(&format!("{}{}", OP_EQ, number)),
            ValueNumber::NotEqual(number) => {
                serializer.serialize_str(&format!("{}{}", OP_NE, number))
            }
            ValueNumber::GreaterThanOrEqual(number) => {
                serializer.serialize_str(&format!("{}{}", OP_GE, number))
            }
            ValueNumber::LessThanOrEqual(number) => {
                serializer.serialize_str(&format!("{}{}", OP_LE, number))
            }
        }
    }
}

impl<'de> Deserialize<'de> for ValueNumber {
    fn deserialize<D>(deserializer: D) -> Result<ValueNumber, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // The order is important.
        // Operators with overlapping characters should have the longer operators
        // first to avoid mix-ups during parsing, e.g. '<=' before '<'.".
        static OPERATORS: [&str; 6] = [OP_GE, OP_LE, OP_EQ, OP_NE, OP_GT, OP_LT];

        let s: &str = Deserialize::deserialize(deserializer)?;
        for operator in OPERATORS.iter() {
            if s.starts_with(operator) {
                let number = s
                    .strip_prefix(operator)
                    .unwrap()
                    .parse()
                    .map_err(serde::de::Error::custom)?;
                match *operator {
                    OP_GE => return Ok(ValueNumber::GreaterThanOrEqual(number)),
                    OP_LE => return Ok(ValueNumber::LessThanOrEqual(number)),
                    OP_EQ => return Ok(ValueNumber::Equal(number)),
                    OP_NE => return Ok(ValueNumber::NotEqual(number)),
                    OP_GT => return Ok(ValueNumber::GreaterThan(number)),
                    OP_LT => return Ok(ValueNumber::LessThan(number)),
                    _ => return Err(serde::de::Error::custom("Invalid operator")),
                }
            }
        }
        Err(serde::de::Error::custom("Invalid operator"))
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn test_matches() {
        let number = super::ValueNumber::Equal(42);
        assert!(number.matches(42));
        assert!(!number.matches(43));

        let number = super::ValueNumber::NotEqual(42);
        assert!(!number.matches(42));
        assert!(number.matches(43));

        let number = super::ValueNumber::GreaterThan(42);
        assert!(!number.matches(42));
        assert!(number.matches(43));

        let number = super::ValueNumber::LessThan(42);
        assert!(number.matches(41));
        assert!(!number.matches(42));

        let number = super::ValueNumber::GreaterThanOrEqual(42);
        assert!(number.matches(42));
        assert!(number.matches(43));

        let number = super::ValueNumber::LessThanOrEqual(42);
        assert!(number.matches(42));
        assert!(number.matches(41));
    }

    #[test]
    fn test_serialization_eq() {
        let number = super::ValueNumber::Equal(42);
        let serialized = serde_json::to_string(&number).unwrap();
        assert_eq!(serialized, "\"=42\"");

        let deserialized: super::ValueNumber = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, number);
    }

    #[test]
    fn test_serialization_ne() {
        let number = super::ValueNumber::NotEqual(42);
        let serialized = serde_json::to_string(&number).unwrap();
        assert_eq!(serialized, "\"!=42\"");

        let deserialized: super::ValueNumber = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, number);
    }

    #[test]
    fn test_serialization_gt() {
        let number = super::ValueNumber::GreaterThan(42);
        let serialized = serde_json::to_string(&number).unwrap();
        assert_eq!(serialized, "\">42\"");

        let deserialized: super::ValueNumber = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, number);
    }

    #[test]
    fn test_serialization_lt() {
        let number = super::ValueNumber::LessThan(42);
        let serialized = serde_json::to_string(&number).unwrap();
        assert_eq!(serialized, "\"<42\"");

        let deserialized: super::ValueNumber = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, number);
    }

    #[test]
    fn test_serialization_ge() {
        let number = super::ValueNumber::GreaterThanOrEqual(42);
        let serialized = serde_json::to_string(&number).unwrap();
        assert_eq!(serialized, "\">=42\"");

        let deserialized: super::ValueNumber = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, number);
    }

    #[test]
    fn test_serialization_le() {
        let number = super::ValueNumber::LessThanOrEqual(42);
        let serialized = serde_json::to_string(&number).unwrap();
        assert_eq!(serialized, "\"<=42\"");

        let deserialized: super::ValueNumber = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, number);
    }
}
