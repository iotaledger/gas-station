// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use fastcrypto::encoding::decode_bytes_hex;
use iota_types::base_types::IotaAddress;
use serde::{
    de::{self, Visitor},
    Deserialize, Serialize,
};

impl ValueIotaAddress {
    pub fn new(addresses: impl IntoIterator<Item = IotaAddress>) -> Self {
        let addresses: Vec<_> = addresses.into_iter().collect();
        if addresses.is_empty() {
            ValueIotaAddress::All
        } else if addresses.len() == 1 {
            ValueIotaAddress::Single(addresses.into_iter().next().unwrap())
        } else {
            ValueIotaAddress::List(addresses)
        }
    }

    pub fn includes(&self, address: &IotaAddress) -> bool {
        match self {
            ValueIotaAddress::All => true,
            ValueIotaAddress::Single(single) => single == address,
            ValueIotaAddress::List(list) => list.contains(address),
        }
    }

    pub fn includes_any<'a>(&self, addresses: impl IntoIterator<Item = &'a IotaAddress>) -> bool {
        addresses.into_iter().any(|address| self.includes(&address))
    }
}

/// The ValueIotaAddress enum represents a single IotaAddress, a list of IotaAddress or all IotaAddresses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ValueIotaAddress {
    #[default]
    All,
    Single(IotaAddress),
    List(Vec<IotaAddress>),
}

impl Serialize for ValueIotaAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        match self {
            ValueIotaAddress::All => serializer.serialize_str("*"),
            ValueIotaAddress::Single(address) => address.serialize(serializer),
            ValueIotaAddress::List(addresses) => addresses.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ValueIotaAddress {
    fn deserialize<D>(deserializer: D) -> Result<ValueIotaAddress, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        struct ValueIotaAddressVisitor;

        impl<'de> Visitor<'de> for ValueIotaAddressVisitor {
            type Value = ValueIotaAddress;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string, a single IotaAddress, or a list of IotaAddresses")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value == "*" {
                    Ok(ValueIotaAddress::All)
                } else {
                    let from_hex: IotaAddress = decode_bytes_hex(value).map_err(E::custom)?;
                    Ok(ValueIotaAddress::Single(from_hex))
                }
            }

            fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let addresses = Vec::deserialize(de::value::SeqAccessDeserializer::new(seq))?;
                Ok(ValueIotaAddress::List(addresses))
            }
        }

        deserializer.deserialize_any(ValueIotaAddressVisitor)
    }
}

impl<K> From<K> for ValueIotaAddress
where
    K: IntoIterator<Item = IotaAddress>,
{
    fn from(k: K) -> Self {
        ValueIotaAddress::new(k)
    }
}

#[cfg(test)]
mod test {
    use iota_types::base_types::IotaAddress;

    use super::ValueIotaAddress;

    #[test]
    fn test_include_from_one() {
        let iota_address = IotaAddress::new([1; 32]);
        let iota_address_not_included = IotaAddress::new([2; 32]);

        let value_iota_address = ValueIotaAddress::from([iota_address]);

        assert!(value_iota_address.includes(&iota_address));
        assert!(!value_iota_address.includes(&iota_address_not_included));
    }

    #[test]
    fn test_include_from_many() {
        let iota_address1 = IotaAddress::new([1; 32]);
        let iota_address2 = IotaAddress::new([2; 32]);
        let iota_address_not_included = IotaAddress::new([3; 32]);

        let value_iota_address = ValueIotaAddress::from([iota_address1, iota_address2]);

        assert!(value_iota_address.includes(&iota_address1));
        assert!(value_iota_address.includes(&iota_address2));
        assert!(!value_iota_address.includes(&iota_address_not_included));
    }

    #[test]
    fn test_serde_one_address() {
        let iota_address = IotaAddress::new([1; 32]);
        let value_iota_address = ValueIotaAddress::Single(iota_address);
        let data = serde_yaml::to_string(&value_iota_address).unwrap();

        assert_eq!(
            "---\n\"0x0101010101010101010101010101010101010101010101010101010101010101\"\n",
            data
        );

        let deserialized_value_iota_address: ValueIotaAddress =
            serde_yaml::from_str(&data).unwrap();
        assert_eq!(value_iota_address, deserialized_value_iota_address);
    }

    #[test]
    fn test_serde_multiple_addresses() {
        let iota_address = IotaAddress::new([1; 32]);
        let value_iota_address = ValueIotaAddress::List(vec![iota_address, iota_address]);
        let data = serde_yaml::to_string(&value_iota_address).unwrap();
        assert_eq!(
            "---\n- \"0x0101010101010101010101010101010101010101010101010101010101010101\"\n- \"0x0101010101010101010101010101010101010101010101010101010101010101\"\n",
            data
        );

        let deserialized_value_iota_address: ValueIotaAddress =
            serde_yaml::from_str(&data).unwrap();
        assert_eq!(value_iota_address, deserialized_value_iota_address);
    }

    #[test]
    fn test_serde_all_addresses() {
        let value_iota_address = ValueIotaAddress::All;
        let data = serde_yaml::to_string(&value_iota_address).unwrap();
        assert_eq!("---\n\"*\"\n", data);

        let deserialized_value_iota_address: ValueIotaAddress =
            serde_yaml::from_str(&data).unwrap();
        assert_eq!(value_iota_address, deserialized_value_iota_address);
    }
}
