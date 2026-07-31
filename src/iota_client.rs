// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! gRPC client wrapper around `iota-sdk-grpc-client`.

use crate::rpc::rpc_types::ExecuteTransactionRequestType;
use crate::types::GasCoin;
use crate::{retry_forever, retry_with_max_attempts};
use iota_sdk_grpc_client::api::Error as GrpcError;
use iota_sdk_grpc_client::read_mask_fields::{SimulateExecutedTransactionField, TransactionField};
use iota_sdk_grpc_client::{Client, HeadersInterceptor, ReadMask};
use iota_sdk_grpc_types::v1::object::Object;
use iota_sdk_transaction_builder::TransactionBuilder;
use iota_sdk_types::{
    Address, Coin, Identifier, Input, ObjectId, SignedTransaction, StructTag, TransactionEffects,
    TypeTag, Version,
};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Per-attempt timeout for a single gRPC call. Without this, a stalled
/// stream (e.g. a fullnode that accepted the connection but never responds)
/// would block a single `retry_forever!`/`retry_with_max_attempts!` attempt
/// forever, instead of ever cycling through the retry strategy -- see
/// `attempt` below.
const GRPC_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Matches the server's own default (`GrpcApiConfig::max_message_size_bytes`
/// in `iota-config`'s `node.rs`, 128 MiB) so large batched responses -- e.g.
/// a 1000-coin `list_owned_objects` page carrying full object BCS -- are not
/// truncated by tonic's 4 MiB default decode limit.
const MAX_DECODING_MESSAGE_SIZE_BYTES: usize = 128 * 1024 * 1024;

/// The server's own maximum page size for `list_owned_objects` (see
/// `MAX_PAGE_SIZE` in `iota-grpc-server`'s `state_service/list_owned_objects.rs`).
/// Strictly better than the old JSON-RPC client's default page size of 50.
const COIN_LIST_PAGE_SIZE: u32 = 1000;

/// Chunk size for batched `get_objects` calls. `get_objects` is a single
/// (non-paginated) call that streams its response, so this isn't required
/// by the API the way `COIN_LIST_PAGE_SIZE` is -- it just bounds how many
/// objects (and how much BCS) go into one request/response pair.
const OBJECT_FETCH_CHUNK_SIZE: usize = 1000;

#[derive(Clone)]
pub struct IotaClient {
    client: Client,
}

/// The public networks' legacy JSON-RPC hosts and the gRPC endpoints that
/// replace them, for [`rewrite_legacy_fullnode_url`].
const LEGACY_JSON_RPC_HOST_TO_GRPC_URL: [(&str, &str); 3] = [
    ("api.mainnet.iota.cafe", "https://grpc.mainnet.iota.cafe"),
    ("api.testnet.iota.cafe", "https://grpc.testnet.iota.cafe"),
    ("api.devnet.iota.cafe", "https://grpc.devnet.iota.cafe"),
];

/// Maps a `fullnode-url` that still points at one of the public networks'
/// legacy JSON-RPC endpoints to the corresponding gRPC endpoint, or `None`
/// if the URL is not a known legacy endpoint (including any self-hosted
/// node, which we cannot guess a gRPC address for).
///
/// This is deliberately applied only here, at the last moment before the
/// connection is created -- everything else in the process (most
/// importantly the Redis storage namespace, which
/// `storage::get_storage_namespace` derives from the *configured* URL)
/// keeps seeing the original value, so a pre-migration config keeps its
/// existing coin registry untouched.
fn rewrite_legacy_fullnode_url(fullnode_url: &str) -> Option<&'static str> {
    let url = url::Url::parse(fullnode_url).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    LEGACY_JSON_RPC_HOST_TO_GRPC_URL
        .iter()
        .find(|(legacy_host, _)| *legacy_host == host)
        .map(|(_, grpc_url)| *grpc_url)
}

impl IotaClient {
    /// Connects to the fullnode's gRPC endpoint.
    ///
    /// A `fullnode_url` still pointing at one of the public networks' legacy
    /// JSON-RPC endpoints is transparently redirected to the matching gRPC
    /// endpoint (with a warning) -- see [`rewrite_legacy_fullnode_url`].
    ///
    /// The underlying channel connects lazily (`Client::new` never touches
    /// the network, see `iota_sdk_grpc_client::Client::new`), so failure
    /// here means the URL/config itself was unusable (bad scheme, HTTPS
    /// requested without the `tls-ring` feature, ...) -- not that the
    /// fullnode was unreachable, which only ever surfaces later from an
    /// actual RPC call.
    pub async fn new(
        fullnode_url: &str,
        basic_auth: Option<(String, String)>,
    ) -> anyhow::Result<Self> {
        let fullnode_url = match rewrite_legacy_fullnode_url(fullnode_url) {
            Some(grpc_url) => {
                warn!(
                    "fullnode-url {fullnode_url:?} is a legacy JSON-RPC endpoint, which the gas \
                     station no longer speaks; connecting to the gRPC endpoint {grpc_url:?} \
                     instead. Please update fullnode-url in your config. Note that changing the \
                     configured URL also changes the Redis storage namespace: the coin registry \
                     is re-initialized from scratch (in-flight reservations are dropped and the \
                     daily gas usage counter resets), so update all instances together during a \
                     full stop, not via a rolling deploy -- see the README section 'Upgrading \
                     from JSON-RPC to gRPC'."
                );
                grpc_url
            }
            None => fullnode_url,
        };
        let mut headers = HeadersInterceptor::new();
        if let Some((username, password)) = basic_auth {
            headers.basic_auth(username, Some(password));
        }
        let client = Client::new(fullnode_url)
            .map_err(|err| anyhow::anyhow!("invalid fullnode gRPC url {fullnode_url:?}: {err}"))?
            .with_headers(headers)
            .with_max_decoding_message_size(MAX_DECODING_MESSAGE_SIZE_BYTES);
        Ok(Self { client })
    }

    pub async fn get_all_owned_iota_coins_above_balance_threshold(
        &self,
        address: Address,
        balance_threshold: u64,
    ) -> Vec<GasCoin> {
        info!(
            "Querying all gas coins owned by sponsor address {} that has at least {} balance",
            address, balance_threshold
        );
        let mut cursor = None;
        let mut coins = Vec::new();
        loop {
            let outcome = retry_forever!(async {
                attempt(async {
                    self.client
                        .list_owned_objects(
                            address,
                            Some(StructTag::new_gas_coin()),
                            Some(COIN_LIST_PAGE_SIZE),
                            cursor.clone(),
                            None,
                        )
                        .await
                })
                .await
            });
            let page = into_anyhow(outcome)
                .unwrap_or_else(|err| panic!("failed to list owned gas coins for {address}: {err}"))
                .into_inner();
            for object in &page.items {
                if let Some(coin) = coin_from_object(object) {
                    if coin.balance >= balance_threshold {
                        coins.push(coin);
                    }
                }
            }
            match page.next_page_token {
                Some(token) => cursor = Some(token),
                None => break,
            }
        }
        coins
    }

    /// Fetches aggregate statistics of all IOTA coins owned by the given address from the network.
    /// Returns (total_coin_count, total_balance) or an error if the network request fails.
    pub async fn get_aggregate_coin_stats(&self, address: Address) -> anyhow::Result<(u64, u64)> {
        debug!(
            "Querying aggregate coin stats for sponsor address {}",
            address
        );
        let mut cursor = None;
        let mut total_count: u64 = 0;
        let mut total_balance: u64 = 0;
        loop {
            let outcome = retry_with_max_attempts!(
                async {
                    attempt(async {
                        self.client
                            .list_owned_objects(
                                address,
                                Some(StructTag::new_gas_coin()),
                                Some(COIN_LIST_PAGE_SIZE),
                                cursor.clone(),
                                None,
                            )
                            .await
                    })
                    .await
                },
                3
            );
            let page = into_anyhow(outcome)
                .map_err(|err| anyhow::anyhow!("failed to get owned gas coins: {err}"))?
                .into_inner();
            for object in &page.items {
                if let Some(coin) = coin_from_object(object) {
                    total_count += 1;
                    total_balance += coin.balance;
                }
            }
            match page.next_page_token {
                Some(token) => cursor = Some(token),
                None => break,
            }
        }
        Ok((total_count, total_balance))
    }

    pub async fn get_reference_gas_price(&self) -> u64 {
        let outcome = retry_forever!(async {
            attempt(async { self.client.get_reference_gas_price().await }).await
        });
        into_anyhow(outcome)
            .unwrap_or_else(|err| panic!("failed to get reference gas price: {err}"))
            .into_inner()
    }

    pub async fn get_latest_gas_objects(
        &self,
        object_ids: impl IntoIterator<Item = ObjectId>,
    ) -> HashMap<ObjectId, Option<GasCoin>> {
        let ids: Vec<ObjectId> = object_ids.into_iter().collect();
        let mut result = HashMap::with_capacity(ids.len());
        for chunk in ids.chunks(OBJECT_FETCH_CHUNK_SIZE) {
            self.fetch_gas_object_chunk(chunk, &mut result).await;
        }
        result
    }

    /// Fetches one chunk via a single batched `get_objects` call.
    ///
    /// `get_objects` returns one result per requested ref, in request order
    /// (`iota-sdk-grpc-client`'s `api/ledger/objects.rs`: results line up
    /// with the requested refs, and a count mismatch fails the call), so no
    /// client-side reordering is needed. A missing gas coin is routine here
    /// (coins get consumed/smashed during execution) and surfaces as a
    /// per-item `NOT_FOUND` in that ref's slot only, leaving the rest of the
    /// batch intact -- it maps to `None`, like any object that isn't a
    /// recognizable gas coin.
    async fn fetch_gas_object_chunk(
        &self,
        chunk: &[ObjectId],
        result: &mut HashMap<ObjectId, Option<GasCoin>>,
    ) {
        let refs: Vec<(ObjectId, Option<Version>)> = chunk.iter().map(|id| (*id, None)).collect();
        let outcome = retry_forever!(async {
            attempt(async { self.client.get_objects(&refs, None).await }).await
        });
        let objects = into_anyhow(outcome)
            .unwrap_or_else(|err| panic!("failed to fetch gas objects: {err}"))
            .into_inner();
        for (id, object) in chunk.iter().zip(objects) {
            let coin = match object {
                Ok(object) => coin_from_object(&object),
                Err(err) if err.is_not_found() => {
                    debug!("Object no longer exists: {:?}", id);
                    None
                }
                Err(err) => panic!("failed to fetch gas object {id}: {err}"),
            };
            result.insert(*id, coin);
        }
    }

    /// Calibrates the gas cost of a single `pay::divide_and_keep` object mutation by
    /// simulating a real split of `gas_coin` and reading back the actual gas
    /// used.
    ///
    /// This is deliberately simulated with an *empty* transaction gas
    /// payment, not a fabricated one: with `skip_checks: true` and
    /// `gas_payment.objects` empty, the authority fabricates its own mock
    /// gas coin owned by `gas_payment.owner`, and with `gas_payment.budget ==
    /// 0` under skipped checks the server rewrites the budget to the
    /// protocol max and reports back the actual cost incurred.
    /// `gas_coin` itself is a real, separate object: it's
    /// the *input* being split by the move call, not the fee-paying gas
    /// object, so its object reference must be real and correct.
    pub async fn calibrate_gas_cost_per_object(
        &self,
        sponsor_address: Address,
        gas_coin: &GasCoin,
        reference_gas_price: u64,
    ) -> u64 {
        const SPLIT_COUNT: u64 = 500;

        let mut builder = TransactionBuilder::new(sponsor_address);
        builder.sponsor(sponsor_address);
        builder.gas_price(reference_gas_price);
        builder.gas_budget(0);
        let coin_arg = builder.input(Input::ImmutableOrOwned(gas_coin.object_ref));
        let count_arg = builder.pure(SPLIT_COUNT);
        builder
            .move_call(ObjectId::FRAMEWORK, Identifier::PAY_MODULE.as_str(), "divide_and_keep")
            .type_tags([TypeTag::from(StructTag::new_gas())])
            .arguments((coin_arg, count_arg));
        // Built entirely offline (no client, no gas objects) -- the only way
        // `finish()` can fail is a missing gas price, which is always set above.
        let tx = builder
            .finish()
            .expect("gas calibration transaction is built offline and always well-formed");

        let outcome = retry_forever!(async {
            attempt(async {
                self.client
                    .simulate_transaction(
                        tx.clone(),
                        true,
                        Some(ReadMask::from(&[SimulateExecutedTransactionField::EFFECTS])),
                    )
                    .await
            })
            .await
        });
        let simulated = into_anyhow(outcome)
            .unwrap_or_else(|err| panic!("failed to simulate gas calibration transaction: {err}"))
            .into_inner();

        let executed = simulated
            .executed_transaction()
            .expect("requested with an explicit `executed_transaction.effects` read mask");
        let effects = executed
            .effects()
            .and_then(|effects| effects.effects())
            .expect("requested with an explicit `executed_transaction.effects` read mask");

        // Multiply by 2 to be conservative and resilient to precision loss.
        gas_used_from_effects(&effects) / SPLIT_COUNT * 2
    }

    /// Executes a fully-signed transaction.
    ///
    /// `checkpoint_inclusion_timeout_ms` is only used when `request_type` is
    /// `WaitForLocalExecution`; it maps to the gRPC call's
    /// `checkpoint_inclusion_timeout_ms`, which asks the server to wait up to
    /// that long for checkpoint inclusion before responding.
    pub async fn execute_transaction(
        &self,
        tx: SignedTransaction,
        max_attempts: usize,
        request_type: Option<ExecuteTransactionRequestType>,
        checkpoint_inclusion_timeout_ms: u64,
    ) -> anyhow::Result<TransactionEffects> {
        let digest = tx.transaction().digest();
        debug!(?digest, "Executing transaction: {:?}", tx);
        let checkpoint_inclusion_timeout_ms =
            match request_type.unwrap_or(ExecuteTransactionRequestType::WaitForEffectsCert) {
                ExecuteTransactionRequestType::WaitForEffectsCert => None,
                ExecuteTransactionRequestType::WaitForLocalExecution => {
                    Some(checkpoint_inclusion_timeout_ms)
                }
            };
        // Narrow read mask: this service only ever reads the digest (for
        // logging) and the effects.
        let mask = ReadMask::from(&[
            TransactionField::TRANSACTION_DIGEST,
            TransactionField::EFFECTS,
        ]);
        let outcome = retry_with_max_attempts!(
            async {
                attempt(async {
                    self.client
                        .execute_transaction(
                            tx.clone(),
                            Some(mask.clone()),
                            checkpoint_inclusion_timeout_ms,
                        )
                        .await
                })
                .await
            },
            max_attempts
        );
        let effects = match into_anyhow(outcome) {
            Ok(envelope) => {
                let executed = envelope.into_inner();
                executed
                    .effects()
                    .and_then(|effects| effects.effects())
                    .map_err(|err| {
                        anyhow::anyhow!(
                            "missing effects in execute_transaction response for {digest}: {err}"
                        )
                    })
            }
            Err(err) => Err(anyhow::anyhow!("execute_transaction error for {digest}: {err}")),
        };
        debug!(?digest, "Transaction execution response: {:?}", effects);
        effects
    }

    /// Waits for a specific object version to become visible on the fullnode.
    #[cfg(test)]
    pub async fn wait_for_object(&self, obj_ref: iota_sdk_types::ObjectReference) {
        loop {
            // A missing object is a per-item error inside an `Ok` envelope,
            // so the call-level `Result` alone doesn't mean "found".
            let found = match self
                .client
                .get_objects(&[(obj_ref.object_id, Some(obj_ref.version))], None)
                .await
            {
                Ok(envelope) => envelope.into_inner().iter().all(|item| item.is_ok()),
                Err(_) => false,
            };
            if found {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

/// Builds a [`GasCoin`] from a `get_objects`/`list_owned_objects` result
/// object, or `None` if it isn't a (recognizable) IOTA gas coin.
fn coin_from_object(object: &Object) -> Option<GasCoin> {
    let object_ref = object.object_reference().ok()?;
    let sdk_object = object.object().ok()?;
    let coin = Coin::try_from_object(&sdk_object).ok()?;
    Some(GasCoin {
        object_ref,
        balance: coin.balance(),
    })
}

/// Extracts the total gas used from a transaction's effects. Split out from
/// `calibrate_gas_cost_per_object` so it can be exercised by a unit test
/// without a live network (see
/// `tests::gas_used_from_effects_reads_gas_cost_summary`).
fn gas_used_from_effects(effects: &TransactionEffects) -> u64 {
    effects.as_v1().gas_cost_summary.gas_used()
}

/// Classifies a gRPC-client error as transient (worth another attempt) or
/// permanent. Bad input, missing objects, and auth/permission failures will
/// never succeed no matter how many times they're retried, so letting
/// `retry_forever!` spin on them would just be a silent hang with extra
/// network chatter; everything else (`Unavailable`, `DeadlineExceeded`,
/// transport resets, ...) is assumed transient.
fn is_transient(err: &GrpcError) -> bool {
    let code = match err {
        GrpcError::Grpc(status) => status.code(),
        GrpcError::Server(status) => tonic::Code::from_i32(status.code),
        // Local/logic errors (proto conversion bugs, an empty id list, a
        // signature that failed to encode, ...) and any future `Error`
        // variant (the enum is `#[non_exhaustive]`) are bugs in this code or
        // a version skew with the SDK, not transient server conditions --
        // retrying can't fix either.
        _ => return false,
    };
    !matches!(
        code,
        tonic::Code::InvalidArgument
            | tonic::Code::NotFound
            | tonic::Code::PermissionDenied
            | tonic::Code::Unauthenticated
            | tonic::Code::FailedPrecondition
            | tonic::Code::Unimplemented
    )
}

/// Runs one gRPC attempt under [`GRPC_CALL_TIMEOUT`] and folds the outcome
/// into the "double `Result`" shape this crate's retry macros need to
/// distinguish "retry me" from "stop, this is final" *without* changing the
/// unconditional-retry-on-`Err` macros themselves (`src/errors.rs`):
/// transient failures come back as the outer `Err` (the macro retries
/// again); permanent failures come back as `Ok(Err(_))` (the macro treats
/// the attempt as done and hands the inner error back to the caller,
/// unretried). Pair with [`into_anyhow`] to unwrap the result after it has
/// passed through a retry macro.
async fn attempt<T>(
    fut: impl std::future::Future<Output = Result<T, GrpcError>>,
) -> Result<Result<T, GrpcError>, GrpcError> {
    match tokio::time::timeout(GRPC_CALL_TIMEOUT, fut).await {
        Ok(Ok(value)) => Ok(Ok(value)),
        Ok(Err(err)) if is_transient(&err) => Err(err),
        Ok(Err(err)) => Ok(Err(err)),
        Err(_elapsed) => Err(GrpcError::Grpc(Box::new(tonic::Status::deadline_exceeded(
            "gas station: internal per-attempt gRPC timeout exceeded",
        )))),
    }
}

/// Flattens the `attempt()` double-`Result` after it has passed through a
/// retry macro: "retries exhausted while still transient" (outer `Err`) and
/// "classified permanent, never retried" (inner `Err`) both become the same
/// `anyhow::Error`, since by this point neither is going to be retried again.
fn into_anyhow<T>(outcome: Result<Result<T, GrpcError>, GrpcError>) -> anyhow::Result<T> {
    match outcome {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) | Err(err) => Err(anyhow::Error::from(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iota_sdk_types::{ExecutionStatus, GasCostSummary, TransactionDigest, TransactionEffectsV1};

    #[test]
    fn gas_used_from_effects_reads_gas_cost_summary() {
        let v1 = TransactionEffectsV1 {
            status: ExecutionStatus::Success,
            epoch: 1,
            gas_cost_summary: GasCostSummary::new(100, 10, 50, 20, 5),
            transaction_digest: TransactionDigest::new([7; 32]),
            gas_object_index: None,
            events_digest: None,
            dependencies: vec![],
            lamport_version: Version::from_u64(1),
            changed_objects: vec![],
            unchanged_shared_objects: vec![],
            auxiliary_data_digest: None,
        };
        let effects = TransactionEffects::V1(Box::new(v1));

        // gas_used() = computation_cost + storage_cost = 100 + 50 = 150,
        // independent of computation_cost_burned/storage_rebate/
        // non_refundable_storage_fee -- pinning this catches an accidental
        // switch to net_gas_usage() (which subtracts storage_rebate) instead.
        assert_eq!(gas_used_from_effects(&effects), 150);
    }

    #[test]
    fn legacy_json_rpc_urls_are_rewritten_to_grpc() {
        for (legacy, grpc) in [
            (
                "https://api.mainnet.iota.cafe",
                "https://grpc.mainnet.iota.cafe",
            ),
            (
                "https://api.testnet.iota.cafe",
                "https://grpc.testnet.iota.cafe",
            ),
            (
                "https://api.devnet.iota.cafe",
                "https://grpc.devnet.iota.cafe",
            ),
        ] {
            assert_eq!(rewrite_legacy_fullnode_url(legacy), Some(grpc));
        }
        // Host matching is scheme/port/path/case-insensitive.
        assert_eq!(
            rewrite_legacy_fullnode_url("https://API.TESTNET.IOTA.CAFE:443/"),
            Some("https://grpc.testnet.iota.cafe")
        );
    }

    #[test]
    fn non_legacy_urls_are_left_untouched() {
        for url in [
            "https://grpc.testnet.iota.cafe",
            "http://localhost:9000",
            "https://fullnode.example.com:9000",
            "not a url at all",
        ] {
            assert_eq!(rewrite_legacy_fullnode_url(url), None);
        }
    }
}
