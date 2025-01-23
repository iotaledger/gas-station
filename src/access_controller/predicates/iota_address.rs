// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use iota_types::base_types::IotaAddress;
use serde::{de::IntoDeserializer, Deserialize, Serialize};

/// The ValueIotaAddress enum represents a single IotaAddress, a list of IotaAddress or all IotaAddresses.
#[derive(Debug, Clone, Default)]
pub enum ValueIotaAddress {
    #[default]
    All,
    Single(IotaAddress),
    List(Vec<IotaAddress>),
}

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
        let value = String::deserialize(deserializer)?;
        if value == "*" {
            Ok(ValueIotaAddress::All)
        } else {
            IotaAddress::deserialize(value.into_deserializer()).map(ValueIotaAddress::Single)
        }
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
            "0x0101010101010101010101010101010101010101010101010101010101010101\n",
            data
        );
    }

    #[test]
    fn test_serde_multiple_addresses() {
        let iota_address = IotaAddress::new([1; 32]);
        let value_iota_address = ValueIotaAddress::List(vec![iota_address, iota_address]);
        let data = serde_yaml::to_string(&value_iota_address).unwrap();
        assert_eq!(
            "- 0x0101010101010101010101010101010101010101010101010101010101010101
- 0x0101010101010101010101010101010101010101010101010101010101010101\n",
            data
        );
    }

    #[test]
    fn test_serde_all_addresses() {
        let value_iota_address = ValueIotaAddress::All;
        let data = serde_yaml::to_string(&value_iota_address).unwrap();
        assert_eq!("'*'\n", data);
    }
}
