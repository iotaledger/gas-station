// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub mod client;
mod rpc_types;
mod server;

pub use server::GasStationServer;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::access_controller::policy::AccessPolicy;
    use crate::access_controller::predicates::{ValueAggregate, ValueNumber};
    use crate::access_controller::rule::AccessRuleBuilder;
    use crate::access_controller::AccessController;
    use crate::config::GasStationConfig;
    use crate::test_env::{
        create_test_transaction, start_rpc_server_for_testing,
        start_rpc_server_for_testing_with_access_controller, DEFAULT_TEST_CONFIG_PATH,
    };
    use crate::AUTH_ENV_NAME;
    use iota_json_rpc_types::IotaTransactionBlockEffectsAPI;
    use iota_types::gas_coin::NANOS_PER_IOTA;

    #[tokio::test]
    async fn test_basic_rpc_flow() {
        let (test_cluster, _container, server) =
            start_rpc_server_for_testing(vec![NANOS_PER_IOTA; 10], NANOS_PER_IOTA).await;
        let client = server.get_local_client();
        client.health().await.unwrap();

        let (sponsor, reservation_id, gas_coins) =
            client.reserve_gas(NANOS_PER_IOTA, 10).await.unwrap();
        assert_eq!(gas_coins.len(), 1);

        // We can no longer request all balance given one is loaned out above.
        assert!(client.reserve_gas(NANOS_PER_IOTA * 10, 10).await.is_err());

        let (tx_data, user_sig) = create_test_transaction(&test_cluster, sponsor, gas_coins).await;
        let effects = client
            .execute_tx(reservation_id, &tx_data, &user_sig)
            .await
            .unwrap();
        assert!(effects.status().is_ok());
    }

    #[tokio::test]
    async fn test_invalid_auth() {
        let (_test_cluster, _container, server) =
            start_rpc_server_for_testing(vec![NANOS_PER_IOTA; 10], NANOS_PER_IOTA).await;

        let client = server.get_local_client();
        client.health().await.unwrap();

        let (_sponsor, _res_id, gas_coins) = client.reserve_gas(NANOS_PER_IOTA, 10).await.unwrap();
        assert_eq!(gas_coins.len(), 1);

        // Change the auth secret used in the client.
        std::env::set_var(AUTH_ENV_NAME, "b");
        assert!(client.reserve_gas(NANOS_PER_IOTA, 10).await.is_err());
    }

    #[tokio::test]
    async fn test_access_denied_from_controller() {
        let (test_cluster, _container, server) =
            start_rpc_server_for_testing_with_access_controller(
                vec![NANOS_PER_IOTA; 10],
                NANOS_PER_IOTA,
                AccessController::new(AccessPolicy::DenyAll, []),
            )
            .await;
        let client = server.get_local_client();
        client.health().await.unwrap();

        let (sponsor, reservation_id, gas_coins) =
            client.reserve_gas(NANOS_PER_IOTA, 10).await.unwrap();
        assert_eq!(gas_coins.len(), 1);

        // We can no longer request all balance given one is loaned out above.
        assert!(client.reserve_gas(NANOS_PER_IOTA * 10, 10).await.is_err());

        let (tx_data, user_sig) = create_test_transaction(&test_cluster, sponsor, gas_coins).await;
        assert!(client
            .execute_tx(reservation_id, &tx_data, &user_sig)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_access_allow_after_ac_reload() {
        let reservation_time_secs = 5;
        let (test_cluster, _container, server) =
            start_rpc_server_for_testing_with_access_controller(
                vec![NANOS_PER_IOTA; 10],
                NANOS_PER_IOTA,
                AccessController::new(AccessPolicy::DenyAll, []),
            )
            .await;

        let client = server.get_local_client();
        client.health().await.unwrap();

        let (sponsor, reservation_id, gas_coins) = client
            .reserve_gas(NANOS_PER_IOTA, reservation_time_secs)
            .await
            .unwrap();
        assert_eq!(gas_coins.len(), 1);

        let (tx_data, user_sig) = create_test_transaction(&test_cluster, sponsor, gas_coins).await;
        assert!(client
            .execute_tx(reservation_id, &tx_data, &user_sig)
            .await
            .is_err());

        let mut gas_station_config = GasStationConfig::default();
        let new_access_controller = AccessController::new(AccessPolicy::AllowAll, []);
        gas_station_config.access_controller = new_access_controller;

        let config_file = std::fs::File::create(DEFAULT_TEST_CONFIG_PATH).unwrap();
        serde_yaml::to_writer(config_file, &gas_station_config).unwrap();

        client.reload_access_controller().await.unwrap();

        let (sponsor, reservation_id, gas_coins) = client
            .reserve_gas(NANOS_PER_IOTA, reservation_time_secs)
            .await
            .unwrap();
        let (tx_data, user_sig) = create_test_transaction(&test_cluster, sponsor, gas_coins).await;

        // After the reload, the access controller should accept all transactions
        assert!(client
            .execute_tx(reservation_id, &tx_data, &user_sig)
            .await
            .is_ok());

        std::fs::remove_file(DEFAULT_TEST_CONFIG_PATH).unwrap();
    }

    #[tokio::test]
    async fn test_access_denied_from_controller_gas_usage() {
        let rules = [AccessRuleBuilder::new()
            .gas_limit(ValueAggregate::new(
                Duration::from_secs(60),
                ValueNumber::GreaterThanOrEqual(10000),
            ))
            .deny()
            .build()];

        let (test_cluster, _container, server) =
            start_rpc_server_for_testing_with_access_controller(
                vec![NANOS_PER_IOTA; 60],
                NANOS_PER_IOTA,
                AccessController::new(AccessPolicy::AllowAll, rules),
            )
            .await;

        let client = server.get_local_client();
        client.health().await.unwrap();
        let (sponsor, reservation_id, gas_coins) =
            client.reserve_gas(NANOS_PER_IOTA, 10).await.unwrap();
        assert_eq!(gas_coins.len(), 1);

        // We can no longer request all balance given one is loaned out above.
        assert!(client.reserve_gas(NANOS_PER_IOTA * 10, 10).await.is_err());

        let (tx_data, user_sig) = create_test_transaction(&test_cluster, sponsor, gas_coins).await;
        // The transaction sets the gas budget to 10000000, which is more than the limit set in the rule.
        assert!(client
            .execute_tx(reservation_id, &tx_data, &user_sig)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_debug_health_check() {
        let (_test_cluster, _container, server) =
            start_rpc_server_for_testing(vec![NANOS_PER_IOTA; 10], NANOS_PER_IOTA).await;

        let client = server.get_local_client();
        client.debug_health_check().await.unwrap();
    }
}
