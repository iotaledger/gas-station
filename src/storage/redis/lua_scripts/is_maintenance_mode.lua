-- Copyright (c) 2025 IOTA Stiftung
-- SPDX-License-Identifier: Apache-2.0

-- Check if the gas station is currently in maintenance mode.
-- Returns 1 if maintenance mode is active, 0 otherwise.
-- The first argument is the namespace.
-- The second argument is the current timestamp.

local namespace = ARGV[1]
local current_time = tonumber(ARGV[2])

local t_maintenance_lock = namespace .. ':maintenance_lock'
local locked_timestamp = redis.call('GET', t_maintenance_lock)

if locked_timestamp == false or tonumber(locked_timestamp) < current_time then
    return 0
else
    return 1
end

