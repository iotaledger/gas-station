-- Copyright (c) 2025 IOTA Stiftung
-- SPDX-License-Identifier: Apache-2.0

-- This script migrates keys from the old namespace format to the new namespace format.
-- Old format: {wallet_address}:{key_name}
-- New format: {namespace}:{key_name}  where namespace = {host}_{port}:{wallet_address}:registry
--
-- The script scans all keys matching the old format pattern and renames them to the new format.
-- It preserves data integrity by:
-- 1. Only migrating if the destination key doesn't exist
-- 2. Using RENAME to atomically move keys
--
-- Arguments:
-- ARGV[1] = old_prefix (e.g., "0x1234...abcd")
-- ARGV[2] = new_namespace (e.g., "api.devnet.iota.cafe_443:0x1234...abcd:registry")
--
-- Returns:
-- A table with {migrated_count, skipped_count, error_count}

local old_prefix = ARGV[1]
local new_namespace = ARGV[2]

-- Define the keys that need to be migrated
-- These are all the Redis keys used by the gas station
local key_suffixes = {
    'available_gas_coins',
    'available_coin_total_balance',
    'available_coin_count',
    'expiration_queue',
    'next_reservation_id',
    'initialized',
    'init_lock',
    'maintenance_lock'
}

local migrated_count = 0
local skipped_count = 0
local error_count = 0

-- Migrate known keys
for _, suffix in ipairs(key_suffixes) do
    local old_key = old_prefix .. ':' .. suffix
    local new_key = new_namespace .. ':' .. suffix

    local exists_old = redis.call('EXISTS', old_key)
    local exists_new = redis.call('EXISTS', new_key)

    if exists_old == 1 then
        if exists_new == 0 then
            -- Safely rename the key
            redis.call('RENAME', old_key, new_key)
            migrated_count = migrated_count + 1
        else
            -- Skip - destination already exists
            skipped_count = skipped_count + 1
        end
    end
end

-- Migrate reservation keys (pattern: {old_prefix}:{reservation_id})
-- These are numeric keys created during reservations
local cursor = "0"
local pattern = old_prefix .. ':[0-9]*'

repeat
    local result = redis.call('SCAN', cursor, 'MATCH', pattern, 'COUNT', 1000)
    cursor = result[1]
    local keys = result[2]

    for _, old_key in ipairs(keys) do
        -- Extract the reservation_id part
        local prefix_len = string.len(old_prefix) + 1
        local suffix = string.sub(old_key, prefix_len + 1)

        -- Check if it's a numeric reservation ID (not one of our known keys)
        if tonumber(suffix) ~= nil then
            local new_key = new_namespace .. ':' .. suffix
            local exists_new = redis.call('EXISTS', new_key)

            if exists_new == 0 then
                redis.call('RENAME', old_key, new_key)
                migrated_count = migrated_count + 1
            else
                skipped_count = skipped_count + 1
            end
        end
    end
until cursor == "0"

-- Set schema_version to 1 in the new namespace to mark it as migrated
local schema_version_key = new_namespace .. ':schema_version'
redis.call('SET', schema_version_key, 1)

return {migrated_count, skipped_count, error_count}

