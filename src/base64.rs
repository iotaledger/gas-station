// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! A local Base64 newtype, replacing `fastcrypto::encoding::Base64`.
//!
//! - Serializes/deserializes as a plain base64 string, matching how this type is used
//!   today in JSON DTOs (e.g. `tx_bytes` / `user_sig` fields).
//! - Derives `schemars::JsonSchema`: since this is a newtype (single-field tuple struct),
//!   schemars generates a schema transparently delegating to the inner `String` field, so
//!   this type appears in the generated public API schema exactly as a `String` would.
//! - Uses the standard, padded base64 alphabet via the `base64ct` crate, which is exactly
//!   what `fastcrypto::encoding::Base64` used under the hood.

use base64ct::Encoding as _;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, JsonSchema)]
#[serde(try_from = "String")]
pub struct Base64(String);

impl Base64 {
    /// Encodes bytes into a base64 string (standard alphabet, padded).
    pub fn encode<T: AsRef<[u8]>>(data: T) -> String {
        base64ct::Base64::encode_string(data.as_ref())
    }

    /// Decodes a base64 string into bytes.
    pub fn decode(s: &str) -> anyhow::Result<Vec<u8>> {
        base64ct::Base64::decode_vec(s).map_err(|_| anyhow::anyhow!("Invalid base64 string"))
    }

    /// Wraps the base64 encoding of the given bytes.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(Self::encode(bytes))
    }

    /// Decodes this Base64 value into bytes.
    pub fn to_vec(&self) -> anyhow::Result<Vec<u8>> {
        Self::decode(&self.0)
    }

    /// Returns the underlying base64-encoded string representation.
    pub fn encoded(&self) -> String {
        self.0.clone()
    }
}

impl TryFrom<String> for Base64 {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        // Error on invalid encoding, matching the previous fastcrypto behavior.
        Self::decode(&value)?;
        Ok(Self(value))
    }
}

impl fmt::Display for Base64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_known_base64_string() {
        // "Hello world!" -> base64 standard, padded. This is the same example used in
        // fastcrypto's own doctest for `Base64::encode`.
        let input = b"Hello world!";
        let expected_b64 = "SGVsbG8gd29ybGQh";

        let encoded = Base64::from_bytes(input);
        assert_eq!(encoded.encoded(), expected_b64);
        assert_eq!(Base64::encode(input), expected_b64);

        let decoded = encoded.to_vec().unwrap();
        assert_eq!(decoded, input);

        let via_try_from = Base64::try_from(expected_b64.to_string()).unwrap();
        assert_eq!(via_try_from, encoded);
        assert_eq!(via_try_from.to_vec().unwrap(), input);
    }

    #[test]
    fn rejects_invalid_base64() {
        assert!(Base64::try_from("not-valid-base64!!".to_string()).is_err());
    }

    #[test]
    fn serializes_as_plain_string() {
        let value = Base64::from_bytes(b"Hello world!");
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"SGVsbG8gd29ybGQh\"");

        let deserialized: Base64 = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, value);
    }
}
