-- Copyright (c) 2025 IOTA Stiftung
-- SPDX-License-Identifier: Apache-2.0

-- This script checks the schema version of the Redis data.
-- The schema version is stored in the key {namespace}:schema_version
--
-- It determines the state by checking:
-- 1. If schema_version key exists in new namespace → returns its value (1 for migrated/new)
-- 2. If old namespace has data but no schema_version → returns 0 (old format, needs migration)
-- 3. If neither exists → returns -1 (fresh installation)
--
-- Arguments:
-- ARGV[1] = old_prefix (e.g., "0x1234...abcd")
-- ARGV[2] = new_namespace (e.g., "api.devnet.iota.cafe_443:0x1234...abcd:registry")
--
-- Returns:
-- 1 (or higher) if schema_version exists (migrated or newly initialized)
-- 0 if old namespace exists without schema_version (needs migration)
-- -1 if no data exists (fresh installation)

local old_prefix = ARGV[1]
local new_namespace = ARGV[2]

-- Check if schema_version exists in the new namespace
local schema_version_key = new_namespace .. ':schema_version'
local schema_version = redis.call('GET', schema_version_key)

if schema_version then
    return tonumber(schema_version)
end

-- No schema_version in new namespace, check if old namespace has data
-- Check if the old initialized key exists
local initialized_key = old_prefix .. ':initialized'
local exists_initialized = redis.call('EXISTS', initialized_key)

if exists_initialized == 1 then
    return 0
end

-- Also check if there are available gas coins (in case initialized flag wasn't set)
local coins_key = old_prefix .. ':available_gas_coins'
local coins_len = redis.call('LLEN', coins_key)

if coins_len > 0 then
    return 0
end

-- Nothing exists - fresh installation
return -1
