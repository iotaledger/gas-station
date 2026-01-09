-- Copyright (c) 2025 IOTA Stiftung
-- SPDX-License-Identifier: Apache-2.0

-- Acquires a maintenance lock for the gas station.
-- Maintenance mode is different from initialization mode:
-- - Init lock prevents concurrent coin splitting operations
-- - Maintenance lock prevents other instances from reserving/modifying coins
--
-- During maintenance mode, other instances should not be able to:
-- - Reserve gas coins
-- - Add new coins to the pool
--
-- The lock is acquired for a certain duration, after which it is released.
-- The duration should be long enough such that the maintenance operation can be completed.
-- If the lock is already acquired, the function returns 0.
-- If the lock is acquired, the function returns 1 and sets the lock's expiration time.
-- The first argument is the namespace.
-- The second argument is the current timestamp.
-- The third argument is the duration for which the lock should be held.

local namespace = ARGV[1]
local current_time = tonumber(ARGV[2])
local lock_duration = tonumber(ARGV[3])

local t_maintenance_lock = namespace .. ':maintenance_lock'
local locked_timestamp = redis.call('GET', t_maintenance_lock)

if locked_timestamp == false or tonumber(locked_timestamp) < current_time then
    redis.call('SET', t_maintenance_lock, current_time + lock_duration)
    return 1
else
    return 0
end

