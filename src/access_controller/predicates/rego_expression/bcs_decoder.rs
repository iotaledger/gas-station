use std::fmt::Display;

use anyhow::{bail, Context};
use iota_sdk::json::{IotaJsonValue, MoveTypeLayout};
use regorus::Value;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BcsDataType {
    String,
    U64,
    Bool,
    Address,
    VectorAddress,
    VectorString,
    VectorU64,
    VectorBool,
}

impl Display for BcsDataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.serialize(f).map_err(|_| std::fmt::Error)
    }
}

impl TryFrom<Value> for BcsDataType {
    type Error = anyhow::Error;
    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::String(s) => BcsDataType::try_from(s.as_ref())
                .map_err(|e| anyhow::anyhow!("Failed to convert string to BCS data type: {}", e)),
            _ => {
                bail!(
                    "Expected a string value for BCS data type, got: {:?}",
                    value
                )
            }
        }
    }
}

impl TryFrom<&str> for BcsDataType {
    type Error = anyhow::Error;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "string" => Ok(BcsDataType::String),
            "u64" => Ok(BcsDataType::U64),
            "bool" => Ok(BcsDataType::Bool),
            "vector_string" => Ok(BcsDataType::VectorString),
            "vector_u64" => Ok(BcsDataType::VectorU64),
            "vector_bool" => Ok(BcsDataType::VectorBool),
            "vector_address" => Ok(BcsDataType::VectorAddress),
            "address" => Ok(BcsDataType::Address), // Handle Address type

            _ => bail!("Unsupported BCS data type: {}", value),
        }
    }
}

pub fn bcs_decode_typed(args: Vec<Value>) -> Result<Value, anyhow::Error> {
    if args.len() != 2 {
        bail!("bcs.decode_typed expects 2 arguments, got {}", args.len());
    }
    let data_type = BcsDataType::try_from(args[1].clone())
        .map_err(|e| anyhow::anyhow!("Unsupported BCS data type: {}", e))?;
    let data_array = args[0]
        .as_array()
        .context("First argument must be an array of bytes")?;
    let mut data_bytes: Vec<u8> = Vec::new();

    for item in data_array {
        let byte = item.as_u8().context("Array items must be u8 values")?;
        data_bytes.push(byte);
    }

    bcs_decode_bytes(&data_bytes, data_type).context("Failed to decode value")
}

fn bcs_decode_bytes(data_bytes: &[u8], data_type: BcsDataType) -> Result<Value, anyhow::Error> {
    match data_type {
        BcsDataType::String => {
            let decoded = IotaJsonValue::from_bcs_bytes(
                Some(&MoveTypeLayout::Vector(MoveTypeLayout::U8.into())),
                data_bytes,
            )?;
            Ok(Value::from(decoded.to_json_value()))
        }
        BcsDataType::U64 => {
            let decoded: u64 = bcs::from_bytes(data_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to decode number: {}", e))?;
            Ok(Value::Number(decoded.into()))
        }
        BcsDataType::Bool => {
            let decoded =
                IotaJsonValue::from_bcs_bytes(Some(&MoveTypeLayout::Bool.into()), data_bytes)?;
            Ok(Value::from(decoded.to_json_value()))
        }
        BcsDataType::Address => {
            let decoded =
                IotaJsonValue::from_bcs_bytes(Some(&MoveTypeLayout::Address.into()), data_bytes)?;
            let address_str = decoded
                .to_iota_address()
                .map_err(|e| anyhow::anyhow!("Failed to convert to Iota address: {}", e))?;
            Ok(Value::from(address_str.to_string()))
        }
        BcsDataType::VectorAddress => {
            let decoded = IotaJsonValue::from_bcs_bytes(
                Some(&MoveTypeLayout::Vector(MoveTypeLayout::Address.into())),
                data_bytes,
            )?;
            Ok(Value::from(decoded.to_json_value()))
        }
        BcsDataType::VectorBool => {
            let decoded = IotaJsonValue::from_bcs_bytes(
                Some(&MoveTypeLayout::Vector(MoveTypeLayout::Bool.into())),
                data_bytes,
            )?;
            Ok(Value::from(decoded.to_json_value()))
        }
        // We don't use the IotaJsonValue here, we use the BCS directly
        // It seems the SDK is inconsistent with nested MoveType declarations.
        BcsDataType::VectorString => {
            let decoded: Vec<String> = bcs::from_bytes(data_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to decode vector of strings: {}", e))?;
            Ok(Value::from(
                decoded
                    .into_iter()
                    .map(|v| Value::from(v.as_str()))
                    .collect::<Vec<_>>(),
            ))
        }
        BcsDataType::VectorU64 => {
            let decoded: Vec<u64> = bcs::from_bytes(data_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to decode vector of strings: {}", e))?;
            Ok(Value::from(
                decoded
                    .into_iter()
                    .map(|v| Value::from(v))
                    .collect::<Vec<_>>(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    // we use a payload that is a original TransactionKind that contains the `Pure` inputs.
    // Its important to use original encoding to ensure that the BCS decoder works correctly.
    const TRANSACTION_KIND_JSON: &str = include_str!("./../test_files/transaction_kind.json");

    use iota_types::transaction::{CallArg, TransactionKind};

    use super::*;
    use std::collections::HashMap;
    fn get_test_data() -> HashMap<Vec<u8>, BcsDataType> {
        let tx_kind = serde_json::from_str::<TransactionKind>(TRANSACTION_KIND_JSON)
            .expect("Failed to parse transaction kind JSON");

        let TransactionKind::ProgrammableTransaction(ptb) = tx_kind else {
            panic!("Expected a ProgrammableTransaction kind");
        };
        let inputs_bytes: Vec<Vec<u8>> = ptb
            .inputs
            .iter()
            .filter_map(|input| {
                if let CallArg::Pure(pure_input) = input {
                    Some(pure_input.clone())
                } else {
                    None
                }
            })
            .collect();

        let mut test_data = HashMap::new();

        test_data.insert(inputs_bytes[0].clone(), BcsDataType::String);
        test_data.insert(inputs_bytes[1].clone(), BcsDataType::U64);
        test_data.insert(inputs_bytes[2].clone(), BcsDataType::Address);
        test_data.insert(inputs_bytes[3].clone(), BcsDataType::Bool);
        test_data.insert(inputs_bytes[4].clone(), BcsDataType::VectorString);
        test_data.insert(inputs_bytes[5].clone(), BcsDataType::VectorAddress);
        test_data.insert(inputs_bytes[6].clone(), BcsDataType::VectorU64);
        test_data.insert(inputs_bytes[7].clone(), BcsDataType::VectorBool);
        test_data
    }

    #[test]
    fn test_bcs_decode_bytes() {
        let test_data = get_test_data();
        for (data_bytes, data_type) in test_data {
            let result = bcs_decode_bytes(&data_bytes, data_type.clone()).unwrap();
            match data_type {
                BcsDataType::String => {
                    matches!(result, Value::String(_));
                    assert_eq!(result, Value::String("hello".into()));
                }
                BcsDataType::U64 => {
                    matches!(result, Value::Number(_));
                    assert_eq!(result, Value::Number(123u64.into()));
                }
                BcsDataType::Bool => {
                    matches!(result, Value::Bool(_));
                    assert_eq!(result, Value::Bool(true));
                }
                BcsDataType::Address => {
                    matches!(result, Value::String(_));
                    assert_eq!(
                        result,
                        Value::String(
                            "0xdb06dbb2fc0f61f4fc292f3dec8e3c397e8a8c8c82311a0679e8e7e0bbfc453d"
                                .into()
                        )
                    );
                }
                BcsDataType::VectorAddress => {
                    matches!(result, Value::Array(_));
                    let vec = result.as_array().unwrap();
                    assert_eq!(vec.len(), 2);
                    assert_eq!(
                        vec[0],
                        Value::String(
                            "0xdb06dbb2fc0f61f4fc292f3dec8e3c397e8a8c8c82311a0679e8e7e0bbfc453d"
                                .into()
                        )
                    );
                    assert_eq!(
                        vec[1],
                        Value::String(
                            "0xdb06dbb2fc0f61f4fc292f3dec8e3c397e8a8c8c82311a0679e8e7e0bbfc453d"
                                .into()
                        )
                    );
                }
                BcsDataType::VectorString => {
                    matches!(result, Value::Array(_));
                    let vec = result.as_array().unwrap();
                    assert_eq!(vec.len(), 2);
                    assert_eq!(vec[0], Value::String("hello".into()));
                    assert_eq!(vec[1], Value::String("world".into()));
                }
                BcsDataType::VectorU64 => {
                    matches!(result, Value::Array(_));
                    let vec = result.as_array().unwrap();
                    assert_eq!(vec.len(), 2);
                    assert_eq!(vec[0], Value::Number(1u64.into()));
                    assert_eq!(vec[1], Value::Number(u64::MAX.into()));
                }
                BcsDataType::VectorBool => {
                    matches!(result, Value::Array(_));
                    let vec = result.as_array().unwrap();
                    assert_eq!(vec.len(), 2);
                    assert_eq!(vec[0], Value::Bool(true));
                    assert_eq!(vec[1], Value::Bool(false));
                }
            }
        }
    }

    #[test]
    fn test_bcs_data_type_from_string() {
        let test_data = vec![
            ("string", BcsDataType::String),
            ("u64", BcsDataType::U64),
            ("bool", BcsDataType::Bool),
            ("vector_string", BcsDataType::VectorString),
            ("vector_u64", BcsDataType::VectorU64),
            ("vector_bool", BcsDataType::VectorBool),
            ("vector_address", BcsDataType::VectorAddress),
            ("address", BcsDataType::Address),
        ];

        for (input, expected) in test_data {
            let result = BcsDataType::try_from(input).unwrap();
            assert_eq!(result, expected);
        }

        // Test unsupported type
        let unsupported_type = "unsupported";
        let result = BcsDataType::try_from(unsupported_type);
        assert!(result.is_err());
    }
}
