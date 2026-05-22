#!/usr/bin/env bash
#
# Repeatedly:
#   1. Generate a new keypair via `./target/debug/tool generate-private-key`
#   2. Replace the `keypair:` line in config2.yaml with the new Base64 key
#   3. Request coins from the IOTA faucet for the new address
#   4. Launch the gas station against config2.yaml
#   5. Capture up to two occurrences of
#        "After add_new_coins. New total balance: X, new coin count: Y"
#      from the gas station log
#   6. Append the address, keys, and captured lines to the results file
#
# Usage:
#   ./test_coin_init.sh [REPEAT_COUNT]
#
# Env overrides:
#   CONFIG_FILE        config to edit / run (default: config2.yaml)
#   RESULTS_FILE       cumulative results (default: coin_init_results.log)
#   TOOL_BIN           tool binary path     (default: ./target/debug/tool)
#   GAS_STATION_BIN    gas station binary   (default: ./target/debug/iota-gas-station)
#   RUST_LOG           gas station log filter
#                      (default: info,iota_gas_station::storage::redis=debug)
#   WAIT_SECONDS       max seconds to wait per iteration for log lines (default: 5)
#   FAUCET_WAIT        seconds to wait after faucet before starting station (default: 8)
#   FLUSH_REDIS        set to 1 to FLUSHDB on redis://localhost:6379 between iterations
#
set -uo pipefail

REPEAT="${1:-1}"
CONFIG_FILE="${CONFIG_FILE:-config2.yaml}"
RESULTS_FILE="${RESULTS_FILE:-coin_init_results.log}"
TOOL_BIN="${TOOL_BIN:-./target/debug/tool}"
GAS_STATION_BIN="${GAS_STATION_BIN:-./target/debug/iota-gas-station}"
RUST_LOG="${RUST_LOG:-info,iota_gas_station::storage::redis=debug}"
WAIT_SECONDS="${WAIT_SECONDS:-5}"
FAUCET_WAIT="${FAUCET_WAIT:-8}"
FLUSH_REDIS="${FLUSH_REDIS:-0}"

PATTERN='After add_new_coins\. New total balance: [0-9]+, new coin count: [0-9]+'

GS_PID=""
cleanup() {
    if [[ -n "$GS_PID" ]] && kill -0 "$GS_PID" 2>/dev/null; then
        kill -TERM "$GS_PID" 2>/dev/null || true
        for _ in 1 2 3 4 5; do
            kill -0 "$GS_PID" 2>/dev/null || break
            sleep 1
        done
        kill -KILL "$GS_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

if [[ ! -x "$TOOL_BIN" ]]; then
    echo "ERROR: tool binary not found or not executable: $TOOL_BIN" >&2
    echo "Build it first with: cargo build --bin tool" >&2
    exit 1
fi
if [[ ! -x "$GAS_STATION_BIN" ]]; then
    echo "ERROR: gas station binary not found or not executable: $GAS_STATION_BIN" >&2
    echo "Build it first with: cargo build --bin iota-gas-station" >&2
    exit 1
fi
if [[ ! -f "$CONFIG_FILE" ]]; then
    echo "ERROR: config file not found: $CONFIG_FILE" >&2
    exit 1
fi

echo "Repeat count: $REPEAT" | tee -a "$RESULTS_FILE"
echo "Config:       $CONFIG_FILE" | tee -a "$RESULTS_FILE"
echo "Results file: $RESULTS_FILE"
echo "RUST_LOG:     $RUST_LOG"
echo

for ((i = 1; i <= REPEAT; i++)); do
    echo "========================================"
    echo "Iteration $i / $REPEAT  ($(date -Iseconds))"
    echo "========================================"

    if [[ "$FLUSH_REDIS" == "1" ]]; then
        echo "[flushing redis at localhost:6379]"
        redis-cli -u redis://localhost:6379 FLUSHDB > /dev/null || echo "  (redis-cli flush failed, continuing)"
    fi

    # 1. Generate a new keypair.
    KEY_OUTPUT="$("$TOOL_BIN" generate-private-key)"
    ADDRESS="$(printf '%s\n' "$KEY_OUTPUT"     | sed -n 's/^IOTA Address: //p')"
    BECH32_KEY="$(printf '%s\n' "$KEY_OUTPUT"  | sed -n 's/^Private key (iotaprivkey): //p')"
    BASE64_KEY="$(printf '%s\n' "$KEY_OUTPUT"  | sed -n 's/^Base64 private key: //p')"

    if [[ -z "$ADDRESS" || -z "$BASE64_KEY" ]]; then
        echo "ERROR: could not parse keypair output:" >&2
        printf '%s\n' "$KEY_OUTPUT" >&2
        exit 1
    fi

    echo "Address:    $ADDRESS"
    echo "Base64 key: $BASE64_KEY"

    # 2. Replace the (uncommented) keypair line in the config.
    sed -i -E "s|^([[:space:]]*)keypair:[[:space:]].*|\1keypair: $BASE64_KEY|" "$CONFIG_FILE"

    # 3. Faucet request.
    echo "Requesting faucet ..."
    if ! iota client faucet --address "$ADDRESS"; then
        echo "  faucet command failed; continuing anyway" >&2
    fi
    echo "Waiting ${FAUCET_WAIT}s for faucet coins to settle ..."
    sleep "$FAUCET_WAIT"

    # 4. Run the gas station; capture stdout+stderr.
    RUN_LOG="$(mktemp -t gas_station_iter_${i}.XXXXXX.log)"
    echo "Launching gas station, log: $RUN_LOG"
    RUST_LOG="$RUST_LOG" "$GAS_STATION_BIN" --config-path "$CONFIG_FILE" \
        > "$RUN_LOG" 2>&1 &
    GS_PID=$!

    # 5. Poll the log for up to two matches of the pattern.
    elapsed=0
    matches=0
    while (( elapsed < WAIT_SECONDS )); do
        if ! kill -0 "$GS_PID" 2>/dev/null; then
            echo "  gas station exited before completing; see $RUN_LOG"
            break
        fi
        matches=$(grep -Ec "$PATTERN" "$RUN_LOG" 2>/dev/null)
        matches=${matches:-0}
        if (( matches >= 2 )); then
            break
        fi
        sleep 2
        elapsed=$((elapsed + 2))
    done

    if (( matches < 2 )); then
        echo "  found $matches match(es) after ${elapsed}s; stopping anyway"
    else
        echo "  found 2 match(es) after ${elapsed}s"
    fi

    # 6. Stop the gas station.
    cleanup
    GS_PID=""

    # 7. Append iteration results to the cumulative log.
    {
        echo "----- Iteration $i  ($(date -Iseconds)) -----"
        echo "Address:    $ADDRESS"
        echo "Bech32 key: $BECH32_KEY"
        echo "Base64 key: $BASE64_KEY"
        echo "Matched log lines (up to 2):"
        if ! grep -E "$PATTERN" "$RUN_LOG" | head -n 2 | sed 's/^/  /'; then
            echo "  <no matches>"
        fi
        echo
    } >> "$RESULTS_FILE"

    echo "Run log retained at: $RUN_LOG"
    echo
done

echo "Done. Cumulative results appended to $RESULTS_FILE"
