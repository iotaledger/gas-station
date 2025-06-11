// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Display;

use anyhow::{bail, Context};
use iota_sdk::json::{IotaJsonValue, MoveTypeLayout};
use regorus::Value;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BcsDataType {
    String,
    U8,
    U16,
    U32,
    U64,
    Bool,
    Address,
    VectorAddress,
    VectorString,
    VectorU8,
    VectorU16,
    VectorU32,
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
            "u8" => Ok(BcsDataType::U8),
            "u16" => Ok(BcsDataType::U16),
            "u32" => Ok(BcsDataType::U32),
            "u64" => Ok(BcsDataType::U64),
            "bool" => Ok(BcsDataType::Bool),
            "vector_string" => Ok(BcsDataType::VectorString),
            "vector_u8" => Ok(BcsDataType::VectorU8),
            "vector_u16" => Ok(BcsDataType::VectorU16),
            "vector_u32" => Ok(BcsDataType::VectorU32),
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
        BcsDataType::U8 => {
            let decoded: u8 = bcs::from_bytes(data_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to decode u8: {}", e))?;
            Ok(Value::Number((decoded as u64).into()))
        }
        BcsDataType::U16 => {
            let decoded: u16 = bcs::from_bytes(data_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to decode u16: {}", e))?;
            Ok(Value::Number((decoded as u64).into()))
        }
        BcsDataType::U32 => {
            let decoded: u32 = bcs::from_bytes(data_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to decode u32: {}", e))?;
            Ok(Value::Number((decoded as u64).into()))
        }
        BcsDataType::U64 => {
            let decoded: u64 = bcs::from_bytes(data_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to decode u64: {}", e))?;
            Ok(Value::Number((decoded as u64).into()))
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
        // We don't use the IotaJsonValue here, we use the BCS directly
        BcsDataType::VectorU8 => {
            let decoded: Vec<u8> = bcs::from_bytes(data_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to decode vector of u8: {}", e))?;
            Ok(Value::from(
                decoded
                    .into_iter()
                    .map(|v| Value::from(v as u64))
                    .collect::<Vec<_>>(),
            ))
        }
        BcsDataType::VectorU16 => {
            let decoded: Vec<u16> = bcs::from_bytes(data_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to decode vector of u16: {}", e))?;
            Ok(Value::from(
                decoded
                    .into_iter()
                    .map(|v| Value::from(v as u64))
                    .collect::<Vec<_>>(),
            ))
        }
        BcsDataType::VectorU32 => {
            let decoded: Vec<u32> = bcs::from_bytes(data_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to decode vector of u32: {}", e))?;
            Ok(Value::from(
                decoded
                    .into_iter()
                    .map(|v| Value::from(v as u64))
                    .collect::<Vec<_>>(),
            ))
        }
        BcsDataType::VectorU64 => {
            let decoded: Vec<u64> = bcs::from_bytes(data_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to decode vector of u64: {}", e))?;
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
        test_data.insert(inputs_bytes[1].clone(), BcsDataType::U8);
        test_data.insert(inputs_bytes[2].clone(), BcsDataType::U16);
        test_data.insert(inputs_bytes[3].clone(), BcsDataType::U32);
        test_data.insert(inputs_bytes[4].clone(), BcsDataType::U64);
        test_data.insert(inputs_bytes[5].clone(), BcsDataType::Address);
        test_data.insert(inputs_bytes[6].clone(), BcsDataType::Bool);
        test_data.insert(inputs_bytes[7].clone(), BcsDataType::VectorString);
        test_data.insert(inputs_bytes[8].clone(), BcsDataType::VectorU8);
        test_data.insert(inputs_bytes[9].clone(), BcsDataType::VectorU16);
        test_data.insert(inputs_bytes[10].clone(), BcsDataType::VectorU32);
        test_data.insert(inputs_bytes[11].clone(), BcsDataType::VectorU64);
        test_data.insert(inputs_bytes[12].clone(), BcsDataType::VectorAddress);
        test_data.insert(inputs_bytes[13].clone(), BcsDataType::VectorBool);
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
                BcsDataType::U8 => {
                    matches!(result, Value::Number(_));
                    assert_eq!(result, Value::Number((u8::MAX as u64).into()));
                }
                BcsDataType::U16 => {
                    matches!(result, Value::Number(_));
                    assert_eq!(result, Value::Number((u16::MAX as u64).into()));
                }
                BcsDataType::U32 => {
                    matches!(result, Value::Number(_));
                    assert_eq!(result, Value::Number((u32::MAX as u64).into()));
                }
                BcsDataType::U64 => {
                    matches!(result, Value::Number(_));
                    assert_eq!(result, Value::Number(u64::MAX.into()));
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
                            "0x32699386a39f53191c4d262157d8520ca4c83fa530dd11cb9e80315aa40af77c"
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
                            "0x32699386a39f53191c4d262157d8520ca4c83fa530dd11cb9e80315aa40af77c"
                                .into()
                        )
                    );
                    assert_eq!(
                        vec[1],
                        Value::String(
                            "0x32699386a39f53191c4d262157d8520ca4c83fa530dd11cb9e80315aa40af77c"
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
                BcsDataType::VectorU8 => {
                    matches!(result, Value::Array(_));
                    let vec = result.as_array().unwrap();
                    assert_eq!(vec.len(), 2);
                    assert_eq!(vec[0], Value::Number((u8::MAX as u64).into()));
                    assert_eq!(vec[1], Value::Number((u8::MIN as u64).into()));
                }
                BcsDataType::VectorU16 => {
                    matches!(result, Value::Array(_));
                    let vec = result.as_array().unwrap();
                    assert_eq!(vec.len(), 2);
                    assert_eq!(vec[0], Value::Number((u16::MAX as u64).into()));
                    assert_eq!(vec[1], Value::Number((u16::MIN as u64).into()));
                }
                BcsDataType::VectorU32 => {
                    matches!(result, Value::Array(_));
                    let vec = result.as_array().unwrap();
                    assert_eq!(vec.len(), 2);
                    assert_eq!(vec[0], Value::Number((u32::MAX as u64).into()));
                    assert_eq!(vec[1], Value::Number((u32::MIN as u64).into()));
                }
                BcsDataType::VectorU64 => {
                    matches!(result, Value::Array(_));
                    let vec = result.as_array().unwrap();
                    assert_eq!(vec.len(), 2);
                    assert_eq!(vec[0], Value::Number(u64::MAX.into()));
                    assert_eq!(vec[1], Value::Number(u64::MIN.into()));
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
            ("u8", BcsDataType::U8),
            ("u16", BcsDataType::U16),
            ("u32", BcsDataType::U32),
            ("u64", BcsDataType::U64),
            ("address", BcsDataType::Address),
            ("bool", BcsDataType::Bool),
            ("vector_string", BcsDataType::VectorString),
            ("vector_u8", BcsDataType::VectorU8),
            ("vector_u16", BcsDataType::VectorU16),
            ("vector_u32", BcsDataType::VectorU32),
            ("vector_u64", BcsDataType::VectorU64),
            ("vector_address", BcsDataType::VectorAddress),
            ("vector_bool", BcsDataType::VectorBool),
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
