// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::gas_station::effects_util::gas_object_reference;
use crate::gas_station::rescan_trigger::RescanGasObjectsTrigger;
use crate::gas_station_initializer::NEW_COIN_BALANCE_FACTOR_THRESHOLD;
use crate::iota_client::IotaClient;
use crate::metrics::GasStationCoreMetrics;
use crate::rpc::rpc_types::{ExecuteTransactionRequestType, ReserveGasRequest};
use crate::storage::Storage;
use crate::tx_signer::TxSigner;
use crate::types::{GasCoin, ReservationID};
use crate::{retry_forever, retry_with_max_attempts};
use anyhow::bail;
use iota_sdk_types::{
    Address, Argument, Command, GasPayment, ObjectId, ObjectReference, ProgrammableTransaction,
    SignedTransaction, Transaction, TransactionEffects, TransactionExpiration, TransactionKind,
    TransactionV1, UserSignature,
};
use std::cmp::min;
use std::sync::Arc;
use std::time::Duration;
use tap::TapFallible;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use super::gas_usage_cap::GasUsageCap;

pub const NANOS_PER_IOTA: u64 = 1_000_000_000;

const EXPIRATION_JOB_INTERVAL: Duration = Duration::from_secs(1);

// 10 mins.
pub const RESERVE_GAS_MAX_DURATION_S: u64 = 10 * 60;

pub struct GasStationContainer {
    inner: Arc<GasStation>,
    _coin_unlocker_task: JoinHandle<()>,
    // This is always Some. It is None only after the drop method is called.
    cancel_sender: Option<tokio::sync::oneshot::Sender<()>>,
}

pub struct GasStation {
    signer: Arc<dyn TxSigner>,
    gas_station_store: Arc<dyn Storage>,
    iota_client: IotaClient,
    metrics: Arc<GasStationCoreMetrics>,
    gas_usage_cap: Arc<GasUsageCap>,
    max_gas_budget: u64,
    checkpoint_inclusion_timeout_ms: u64,
    rescan_config: RescanGasObjectsTrigger,
}

impl GasStation {
    pub async fn new(
        signer: Arc<dyn TxSigner>,
        gas_station_store: Arc<dyn Storage>,
        iota_client: IotaClient,
        metrics: Arc<GasStationCoreMetrics>,
        gas_usage_cap: Arc<GasUsageCap>,
        max_gas_budget: u64,
        checkpoint_inclusion_timeout_ms: u64,
        rescan_config: RescanGasObjectsTrigger,
    ) -> Arc<Self> {
        let pool = Self {
            signer,
            gas_station_store,
            iota_client,
            metrics,
            gas_usage_cap,
            max_gas_budget,
            checkpoint_inclusion_timeout_ms,
            rescan_config,
        };

        Arc::new(pool)
    }

    pub async fn reserve_gas(
        &self,
        gas_budget: u64,
        duration: Duration,
    ) -> anyhow::Result<(Address, ReservationID, Vec<ObjectReference>)> {
        let cur_time = std::time::Instant::now();
        self.gas_usage_cap.check_usage().await?;
        let sponsor = self.signer.get_address();
        let (reservation_id, gas_coins) = self
            .gas_station_store
            .reserve_gas_coins(gas_budget, duration.as_millis() as u64)
            .await?;
        let elapsed = cur_time.elapsed().as_millis();
        self.metrics.reserve_gas_latency_ms.observe(elapsed as f64);
        self.metrics
            .reserved_gas_coin_count_per_request
            .observe(gas_coins.len() as f64);
        Ok((
            sponsor,
            reservation_id,
            gas_coins.into_iter().map(|c| c.object_ref).collect(),
        ))
    }

    pub async fn execute_transaction(
        &self,
        reservation_id: ReservationID,
        tx: Transaction,
        user_sig: UserSignature,
        request_type: Option<ExecuteTransactionRequestType>,
    ) -> anyhow::Result<TransactionEffects> {
        let sponsor = tx.as_v1().gas_payment.owner;
        if !self.signer.is_valid_address(&sponsor) {
            bail!("Sponsor {:?} is not registered", sponsor);
        };
        Self::check_transaction_validity(&tx)?;
        let payment: Vec<ObjectId> = tx
            .as_v1()
            .gas_payment
            .objects
            .iter()
            .map(|oref| oref.object_id)
            .collect();
        let payment_count = payment.len();
        debug!(
            ?reservation_id,
            "Payment coins in transaction: {:?}", payment
        );
        self.gas_station_store
            .ready_for_execution(reservation_id)
            .await?;
        debug!(?reservation_id, "Reservation is ready for execution");

        // To avoid read-after-write inconsistency, we apply a trick here to calculate the
        // new balance of the gas coin after the transaction.
        // We first query the total balance prior to transaction execution, then execute the
        // transaction, and finally derive the new gas coin balance using the gas usage from effects.
        let total_gas_coin_balance = self.get_total_gas_coin_balance(payment.clone()).await;
        debug!(
            ?reservation_id,
            "Total gas coin balance prior to execution: {}", total_gas_coin_balance,
        );
        let response = self
            .execute_transaction_impl(reservation_id, tx, user_sig, request_type)
            .await;
        let updated_coins = match &response {
            Ok(effects) => {
                let new_gas_coin = gas_object_reference(effects);
                let net_gas_usage = effects.as_v1().gas_cost_summary.net_gas_usage();
                // Computed in i128 because neither operand's range fits the
                // result: `total_gas_coin_balance` is a u64 (so values above
                // i64::MAX are misread as negative by `as i64`), and
                // `net_gas_usage` is a signed i64 that is *legitimately*
                // negative whenever gas smashing refunds more storage than the
                // transaction spent. i128 holds every (u64, i64) pair exactly,
                // so the single conversion below is the only place either
                // direction can be rejected.
                let new_balance: i128 =
                    total_gas_coin_balance as i128 - net_gas_usage as i128;
                debug!(
                    ?reservation_id,
                    "New gas coin balance after execution: {}", new_balance,
                );
                #[cfg(test)]
                {
                    self.iota_client.wait_for_object(new_gas_coin).await;
                    assert_eq!(
                        i128::from(self.get_total_gas_coin_balance(payment).await),
                        new_balance
                    );
                }
                self.metrics
                    .gas_usage_per_transaction
                    .observe(net_gas_usage as f64);
                self.metrics
                    .reserved_gas_real_gas_usage_delta
                    // If a refund occurs, ensure that the delta does not exceed the original total balance of the gas coins
                    .observe(min(new_balance, total_gas_coin_balance as i128) as f64);

                // A successful transaction cannot leave its own gas coin with a
                // negative balance: the validators debited this very coin and a
                // `Coin<IOTA>` cannot go below zero. So a value that does not
                // fit a u64 is not a gas-accounting outcome, it is proof that
                // `total_gas_coin_balance` was wrong — in practice because the
                // pre-execution read could not resolve the coin and reported 0.
                //
                // Record it and release the coin at a balance of zero rather
                // than storing the wrapped value. `ready_for_execution` has
                // already removed this coin from the pool and `expire_coins`
                // cannot surface it again, so withholding it here would lose it
                // permanently; an understated balance is recoverable and
                // self-heals on the coin's next successful use.
                let balance = u64::try_from(new_balance).unwrap_or_else(|_| {
                    self.metrics.invariant_violation(format!(
                        "derived balance {new_balance} is not a valid u64 for gas coin {} \
                         (pre-execution total {total_gas_coin_balance}, net gas usage \
                         {net_gas_usage}); releasing the coin with a recorded balance of 0",
                        new_gas_coin.object_id
                    ));
                    0
                });
                vec![GasCoin {
                    object_ref: new_gas_coin,
                    balance,
                }]
            }
            Err(_) => {
                debug!(
                    ?reservation_id,
                    "Querying latest gas state since transaction failed"
                );
                self.iota_client
                    .get_latest_gas_objects(payment)
                    .await
                    .into_values()
                    .flatten()
                    .collect()
            }
        };
        let smashed_coin_count = payment_count - updated_coins.len();
        // Before returning the coins to the pool, verify if any coin exceeds
        // NEW_COIN_BALANCE_FACTOR_THRESHOLD times the target initial balance.
        let oversized_coins_count = updated_coins
            .iter()
            .filter(|coin| {
                coin.balance
                    > NEW_COIN_BALANCE_FACTOR_THRESHOLD * self.rescan_config.target_init_balance
            })
            .count();
        if oversized_coins_count > 0 {
            warn!("Oversized coins found during transaction execution. Initiating rescan to split these coins. If this occurs frequently, consider adjusting target_init_balance or the maximum transaction budget.");
            self.rescan_config.trigger_rescan().await;
            self.metrics
                .oversized_gas_coins_count
                .with_label_values(&[&sponsor.to_string()])
                .inc_by(oversized_coins_count as u64);
        } else {
            // Regardless of whether the transaction succeeded, we need to release the coins.
            // Otherwise, we lose track of them. This is because `ready_for_execution` already takes
            // the coins out of the pool and will not be covered by the auto-release mechanism.
            info!(
                ?reservation_id,
                "Releasing {} coins back to the pool",
                updated_coins.len()
            );
            self.release_gas_coins(updated_coins).await;
        }
        if smashed_coin_count > 0 {
            info!(
                ?reservation_id,
                "Smashed {:?} coins after transaction execution", smashed_coin_count
            );
            self.metrics
                .num_smashed_gas_coins
                .with_label_values(&[&sponsor.to_string()])
                .inc_by(smashed_coin_count as u64);
        }
        info!(?reservation_id, "Transaction execution finished");

        response
    }

    async fn execute_transaction_impl(
        &self,
        reservation_id: ReservationID,
        tx: Transaction,
        user_sig: UserSignature,
        request_type: Option<ExecuteTransactionRequestType>,
    ) -> anyhow::Result<TransactionEffects> {
        let sponsor = tx.as_v1().gas_payment.owner;
        let cur_time = std::time::Instant::now();
        let sponsor_sig = retry_with_max_attempts!(
            async {
                self.signer
                    .sign_transaction(&tx)
                    .await
                    .tap_err(|err| error!("Failed to sign transaction: {:?}", err))
            },
            3
        )?;
        let elapsed = cur_time.elapsed().as_millis();
        self.metrics
            .transaction_signing_latency_ms
            .observe(elapsed as f64);
        debug!(?reservation_id, "Transaction signed by sponsor");

        let signed_tx = SignedTransaction {
            transaction: tx,
            signatures: vec![sponsor_sig, user_sig],
        };
        let cur_time = std::time::Instant::now();
        let effects = self
            .iota_client
            .execute_transaction(
                signed_tx,
                3,
                request_type,
                self.checkpoint_inclusion_timeout_ms,
            )
            .await?;
        debug!(?reservation_id, "Transaction executed");
        let elapsed = cur_time.elapsed().as_millis();
        self.metrics
            .transaction_execution_latency_ms
            .observe(elapsed as f64);
        let net_gas_usage = effects.as_v1().gas_cost_summary.net_gas_usage();
        let new_daily_usage = self.gas_usage_cap.update_usage(net_gas_usage).await;
        self.metrics
            .daily_gas_usage
            .with_label_values(&[&sponsor.to_string()])
            .set(new_daily_usage);
        Ok(effects)
    }

    async fn get_total_gas_coin_balance(&self, gas_coins: Vec<ObjectId>) -> u64 {
        let latest = self.iota_client.get_latest_gas_objects(gas_coins).await;
        latest
            .into_values()
            .flatten()
            .map(|coin| coin.balance)
            .sum()
    }

    /// Checks gas reservation request validity.
    /// Note that this is intended as a pre-flight check in server request handling.
    /// Calling `GasStation::reserve_gas` directly will allow to use values outside of the boundaries checked here.
    pub fn check_reserve_gas_request_validity(
        &self,
        request: &ReserveGasRequest,
    ) -> anyhow::Result<()> {
        if request.gas_budget == 0 {
            anyhow::bail!("Gas budget must be positive");
        }
        if request.gas_budget > self.max_gas_budget {
            anyhow::bail!("Gas budget must be less than {}", self.max_gas_budget);
        }
        if request.reserve_duration_secs == 0 {
            anyhow::bail!("Reserve duration must be positive");
        }
        if request.reserve_duration_secs > RESERVE_GAS_MAX_DURATION_S {
            anyhow::bail!(
                "Reserve duration must be less than {} seconds",
                RESERVE_GAS_MAX_DURATION_S
            );
        }
        Ok(())
    }

    /// Checks that no command in the transaction uses the gas coin as a
    /// regular argument -- only this service's own machinery is allowed to
    /// spend it, to pay for the transaction itself.
    ///
    /// # Security note: fails *closed* on unrecognized commands
    ///
    /// `iota_sdk_types::Command` is `#[non_exhaustive]`: the SDK can add new
    /// command variants in a future revision without that being a breaking
    /// change for this crate. Because this is a *validity check* -- its whole
    /// purpose is to reject unsafe transactions -- an unrecognized variant is
    /// treated as unsafe **by default** (rejected) rather than silently
    /// passed through. A permissive `_ => Ok(())`-style wildcard would fail
    /// *open*: any future command variant would be silently accepted here
    /// even though this function has never been taught whether it can
    /// reference the gas coin in some new way.
    ///
    /// # Pre-existing gaps, preserved exactly as they were before this
    /// migration (flagged, not fixed -- see this migration stage's report)
    ///
    /// This mirrors the pre-migration logic's scan exactly, including three
    /// cases it does *not* cover, each carried over unchanged rather than
    /// tightened as part of this type-system migration:
    /// - `Command::TransferObjects`'s destination `address` argument is not
    ///   scanned (only the `objects` being transferred are).
    /// - `Command::SplitCoins`'s `amounts` arguments are not scanned (only
    ///   the `coin` being split is).
    /// - `Command::Upgrade`'s `ticket` argument is not scanned at all.
    fn check_transaction_validity(tx: &Transaction) -> anyhow::Result<()> {
        let commands: &[Command] = match &tx.as_v1().kind {
            TransactionKind::Programmable(pt) => &pt.commands,
            // Non-programmable transaction kinds (system transactions) carry
            // no commands and so can never reference the gas coin as an
            // `Argument`; matches the pre-migration
            // `TransactionKind::iter_commands()`, which yielded an empty
            // iterator for every non-`ProgrammableTransaction` kind.
            _ => &[],
        };
        let mut all_args = vec![];
        for command in commands {
            match command {
                Command::MoveCall(call) => {
                    all_args.extend(call.arguments.iter());
                }
                Command::TransferObjects(t) => {
                    all_args.extend(t.objects.iter());
                }
                Command::SplitCoins(s) => {
                    all_args.push(&s.coin);
                }
                Command::MergeCoins(m) => {
                    all_args.push(&m.coin);
                    all_args.extend(m.coins_to_merge.iter());
                }
                Command::Publish(_) => {}
                Command::MakeMoveVector(v) => {
                    all_args.extend(v.elements.iter());
                }
                Command::Upgrade(_) => {}
                // Deny-by-default for any command variant this function does
                // not (yet) recognize -- see the doc comment above.
                _ => bail!(
                    "Unrecognized transaction command variant; rejecting as unsafe by default"
                ),
            };
        }
        let uses_gas = all_args
            .into_iter()
            .any(|arg| matches!(*arg, Argument::Gas));
        if uses_gas {
            bail!("Gas coin can only be used to pay gas")
        };
        Ok(())
    }

    /// Release gas coins back to the Gas Station, by adding them to the storage.
    async fn release_gas_coins(&self, gas_coins: Vec<GasCoin>) {
        debug!("Trying to release gas coins: {:?}", gas_coins);
        retry_forever!(async {
            self.gas_station_store
                .add_new_coins(gas_coins.clone())
                .await
                .tap_err(|err| error!("Failed to call update_gas_coins on storage: {:?}", err))
        })
        .unwrap();
    }

    /// Performs an end-to-end flow of reserving gas, signing a transaction, and releasing the gas coins.
    pub async fn debug_check_health(&self) -> anyhow::Result<()> {
        let gas_budget = NANOS_PER_IOTA / 10;
        let (_address, _reservation_id, gas_coins) =
            self.reserve_gas(gas_budget, Duration::from_secs(3)).await?;
        let tx_kind = TransactionKind::Programmable(ProgrammableTransaction {
            inputs: vec![],
            commands: vec![],
        });
        // Since we just want to check the health of the signer, we don't need to actually execute the transaction.
        let tx = Transaction::V1(TransactionV1 {
            kind: tx_kind,
            sender: Address::ZERO,
            gas_payment: GasPayment {
                objects: gas_coins,
                owner: Address::ZERO,
                price: 0,
                budget: gas_budget,
            },
            expiration: TransactionExpiration::None,
        });
        self.signer.sign_transaction(&tx).await?;
        Ok(())
    }

    async fn start_coin_unlock_task(
        self: Arc<Self>,
        mut cancel_receiver: tokio::sync::oneshot::Receiver<()>,
    ) -> JoinHandle<()> {
        tokio::task::spawn(async move {
            loop {
                let expire_results = self.gas_station_store.expire_coins().await;
                let unlocked_coins = expire_results.unwrap_or_else(|err| {
                    error!("Failed to call expire_coins to the storage: {:?}", err);
                    vec![]
                });
                if !unlocked_coins.is_empty() {
                    debug!("Coins that are expired: {:?}", unlocked_coins);
                    let latest_coins: Vec<_> = self
                        .iota_client
                        .get_latest_gas_objects(unlocked_coins.clone())
                        .await
                        .into_values()
                        .flatten()
                        .collect();
                    let count = latest_coins.len();
                    self.release_gas_coins(latest_coins).await;
                    info!("Released {:?} coins after expiration", count);
                }
                tokio::select! {
                    _ = tokio::time::sleep(EXPIRATION_JOB_INTERVAL) => {}
                    _ = &mut cancel_receiver => {
                        info!("Coin unlocker task is cancelled");
                        break;
                    }
                }
            }
        })
    }

    pub async fn query_pool_available_coin_count(&self) -> usize {
        self.gas_station_store
            .get_available_coin_count()
            .await
            .unwrap()
    }
}

impl GasStationContainer {
    pub async fn new(
        signer: Arc<dyn TxSigner>,
        gas_station_store: Arc<dyn Storage>,
        iota_client: IotaClient,
        gas_usage_daily_cap: u64,
        max_gas_budget: u64,
        checkpoint_inclusion_timeout_ms: u64,
        metrics: Arc<GasStationCoreMetrics>,
        rescan_config: RescanGasObjectsTrigger,
    ) -> Self {
        let inner = GasStation::new(
            signer,
            gas_station_store,
            iota_client,
            metrics,
            Arc::new(GasUsageCap::new(gas_usage_daily_cap)),
            max_gas_budget,
            checkpoint_inclusion_timeout_ms,
            rescan_config,
        )
        .await;
        let (cancel_sender, cancel_receiver) = tokio::sync::oneshot::channel();
        let _coin_unlocker_task = inner.clone().start_coin_unlock_task(cancel_receiver).await;

        Self {
            inner,
            _coin_unlocker_task,
            cancel_sender: Some(cancel_sender),
        }
    }

    pub fn get_gas_station_arc(&self) -> Arc<GasStation> {
        self.inner.clone()
    }

    #[cfg(test)]
    pub fn get_signer_address(&self) -> Address {
        self.inner.signer.get_address()
    }
}

impl Drop for GasStationContainer {
    fn drop(&mut self) {
        self.cancel_sender.take().unwrap().send(()).unwrap();
    }
}
