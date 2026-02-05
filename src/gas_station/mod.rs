// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub mod gas_station_core;
mod gas_usage_cap;
pub(crate) mod rescan_trigger;

#[cfg(test)]
mod tests {
    use crate::test_env::{create_test_transaction, start_gas_station};
    use iota_json_rpc_types::IotaTransactionBlockEffectsAPI;
    use iota_sdk_types::Intent;
    use iota_sdk_types::IntentMessage;
    use iota_types::{
        crypto::{get_account_key_pair, Signature},
        gas_coin::NANOS_PER_IOTA,
        programmable_transaction_builder::ProgrammableTransactionBuilder,
        transaction::{TransactionData, TransactionKind},
    };
    use std::time::Duration;

    #[tokio::test]
    async fn test_station_reserve_gas() {
        let (_test_cluster, container) =
            start_gas_station(vec![NANOS_PER_IOTA; 10], NANOS_PER_IOTA, None).await;
        let station = container.get_gas_station_arc();
        let (sponsor1, _res_id1, gas_coins) = station
            .reserve_gas(NANOS_PER_IOTA * 3, Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(gas_coins.len(), 3);
        assert_eq!(station.query_pool_available_coin_count().await, 7);
        let (sponsor2, _res_id2, gas_coins) = station
            .reserve_gas(NANOS_PER_IOTA * 7, Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(gas_coins.len(), 7);
        assert_eq!(sponsor1, sponsor2);
        assert_eq!(station.query_pool_available_coin_count().await, 0);
        assert!(station
            .reserve_gas(1, Duration::from_secs(10))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_e2e_gas_station_flow() {
        let (test_cluster, container) =
            start_gas_station(vec![NANOS_PER_IOTA], NANOS_PER_IOTA, None).await;
        let station = container.get_gas_station_arc();
        assert!(station
            .reserve_gas(NANOS_PER_IOTA + 1, Duration::from_secs(10))
            .await
            .is_err());

        let (sponsor, reservation_id, gas_coins) = station
            .reserve_gas(NANOS_PER_IOTA, Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(gas_coins.len(), 1);
        assert_eq!(station.query_pool_available_coin_count().await, 0);
        assert!(station
            .reserve_gas(1, Duration::from_secs(10))
            .await
            .is_err());

        let (tx_data, user_sig) = create_test_transaction(&test_cluster, sponsor, gas_coins).await;
        let effects = station
            .execute_transaction(reservation_id, tx_data, user_sig, None)
            .await
            .unwrap();
        assert!(effects.status().is_ok());
        assert_eq!(station.query_pool_available_coin_count().await, 1);
    }

    #[tokio::test]
    async fn test_invalid_transaction() {
        telemetry_subscribers::init_for_testing();
        let (_test_cluster, container) =
            start_gas_station(vec![NANOS_PER_IOTA], NANOS_PER_IOTA, None).await;
        let station = container.get_gas_station_arc();
        let (sponsor, reservation_id, gas_coins) = station
            .reserve_gas(NANOS_PER_IOTA, Duration::from_secs(10))
            .await
            .unwrap();
        let (sender, keypair) = get_account_key_pair();
        let tx_kind = TransactionKind::programmable(ProgrammableTransactionBuilder::new().finish());
        let tx_data = TransactionData::new_with_gas_coins_allow_sponsor(
            tx_kind, sender, gas_coins, 1, 1, sponsor,
        );
        let user_sig = Signature::new_secure(
            &IntentMessage::new(Intent::iota_transaction(), &tx_data),
            &keypair,
        );
        let result = station
            .execute_transaction(reservation_id, tx_data, user_sig.into(), None)
            .await;
        println!("{:?}", result);
        assert!(result.is_err());
        assert_eq!(station.query_pool_available_coin_count().await, 1);
    }

    #[tokio::test]
    async fn test_coin_expiration() {
        telemetry_subscribers::init_for_testing();
        let (test_cluster, container) =
            start_gas_station(vec![NANOS_PER_IOTA], NANOS_PER_IOTA, None).await;
        let station = container.get_gas_station_arc();
        let (sponsor, reservation_id, gas_coins) = station
            .reserve_gas(NANOS_PER_IOTA, Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(gas_coins.len(), 1);
        assert_eq!(station.query_pool_available_coin_count().await, 0);
        assert!(station
            .reserve_gas(1, Duration::from_secs(1))
            .await
            .is_err());
        // Sleep a little longer to give it enough time to expire.
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(station.query_pool_available_coin_count().await, 1);
        let (tx_data, user_sig) = create_test_transaction(&test_cluster, sponsor, gas_coins).await;
        assert!(station
            .execute_transaction(reservation_id, tx_data, user_sig, None)
            .await
            .is_err());
        station
            .reserve_gas(1, Duration::from_secs(1))
            .await
            .unwrap();
    }

    #[ignore]
    #[tokio::test]
    async fn test_incomplete_gas_usage() {
        let (test_cluster, container) =
            start_gas_station(vec![NANOS_PER_IOTA; 10], NANOS_PER_IOTA, None).await;
        let station = container.get_gas_station_arc();
        let (sponsor, reservation_id, gas_coins) = station
            .reserve_gas(NANOS_PER_IOTA * 3, Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(gas_coins.len(), 3);

        // Remove one gas object from the reserved list and only use the two.
        let mut incomplete_gas_coins = gas_coins.clone();
        incomplete_gas_coins.pop().unwrap();
        let (tx_data, user_sig) =
            create_test_transaction(&test_cluster, sponsor, incomplete_gas_coins).await;
        // It should fail because it's inconsistent with the reservation.
        assert!(station
            .execute_transaction(reservation_id, tx_data, user_sig, None)
            .await
            .is_err());

        let (tx_data, user_sig) = create_test_transaction(&test_cluster, sponsor, gas_coins).await;
        let effects = station
            .execute_transaction(reservation_id, tx_data, user_sig, None)
            .await
            .unwrap();
        assert!(effects.status().is_ok());
    }

    #[ignore]
    #[tokio::test]
    async fn test_mixed_up_gas_coins() {
        let (test_cluster, container) =
            start_gas_station(vec![NANOS_PER_IOTA; 10], NANOS_PER_IOTA, None).await;
        let station = container.get_gas_station_arc();
        let (sponsor, reservation_id1, gas_coins1) = station
            .reserve_gas(NANOS_PER_IOTA * 3, Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(gas_coins1.len(), 3);
        let (_, _res_id2, gas_coins2) = station
            .reserve_gas(NANOS_PER_IOTA, Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(gas_coins2.len(), 1);

        // Mix up gas coins from two reservations.
        let mut mixed_up_gas_coins = gas_coins1.clone();
        mixed_up_gas_coins[0] = gas_coins2[0];
        let (tx_data, user_sig) =
            create_test_transaction(&test_cluster, sponsor, mixed_up_gas_coins).await;
        assert!(station
            .execute_transaction(reservation_id1, tx_data, user_sig, None)
            .await
            .is_err());

        let (tx_data, user_sig) = create_test_transaction(&test_cluster, sponsor, gas_coins1).await;
        let effects = station
            .execute_transaction(reservation_id1, tx_data, user_sig, None)
            .await
            .unwrap();
        assert!(effects.status().is_ok());
    }

    mod check_reserve_gas_request_validity {
        use super::*;

        use crate::{config::DEFAULT_MAX_GAS_BUDGET, rpc::rpc_types::ReserveGasRequest};

        struct Spec {
            gas_budget: u64,
            is_ok: bool,
        }

        impl Spec {
            pub fn new(gas_budget: u64, is_ok: bool) -> Self {
                Self { gas_budget, is_ok }
            }
        }

        async fn run_test_for_max_gas_budget(max_gas_budget: u64) {
            let test_set = vec![
                Spec::new(max_gas_budget, true),
                Spec::new(max_gas_budget - 1, true),
                Spec::new(max_gas_budget + 1, false),
                Spec::new(0, false),
            ];
            let (_test_cluster, container) = start_gas_station(
                vec![NANOS_PER_IOTA; 10],
                NANOS_PER_IOTA,
                Some(max_gas_budget),
            )
            .await;
            let station = container.get_gas_station_arc();

            for Spec { gas_budget, is_ok } in test_set.into_iter() {
                let request = ReserveGasRequest {
                    gas_budget,
                    reserve_duration_secs: 10,
                };
                let result = station.check_reserve_gas_request_validity(&request);
                assert_eq!(result.is_ok(), is_ok);
            }
        }

        #[tokio::test]
        async fn with_default_max_gas_budget() {
            run_test_for_max_gas_budget(DEFAULT_MAX_GAS_BUDGET).await;
        }

        #[tokio::test]
        async fn with_lower_max_gas_budget() {
            run_test_for_max_gas_budget(DEFAULT_MAX_GAS_BUDGET - 1000).await;
        }

        #[tokio::test]
        async fn with_larger_gas_budget() {
            run_test_for_max_gas_budget(DEFAULT_MAX_GAS_BUDGET + 1000).await;
        }
    }
}
