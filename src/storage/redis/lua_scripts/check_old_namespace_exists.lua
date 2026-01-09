-- Copyright (c) 2025 IOTA Stiftung
-- SPDX-License-Identifier: Apache-2.0

-- This script checks if the old namespace format exists.
-- It looks for the key {wallet_address}:initialized or {wallet_address}:available_gas_coins
-- to determine if migration is needed.
--
-- Arguments:
-- ARGV[1] = old_prefix (e.g., "0x1234...abcd")
--
-- Returns:
-- 1 if old namespace exists and has data, 0 otherwise

local old_prefix = ARGV[1]

-- Check if the old initialized key exists
local initialized_key = old_prefix .. ':initialized'
local exists_initialized = redis.call('EXISTS', initialized_key)

if exists_initialized == 1 then
    return 1
end

-- Also check if there are available gas coins (in case initialized flag wasn't set)
local coins_key = old_prefix .. ':available_gas_coins'
local coins_len = redis.call('LLEN', coins_key)

if coins_len > 0 then
    return 1
end

return 0

