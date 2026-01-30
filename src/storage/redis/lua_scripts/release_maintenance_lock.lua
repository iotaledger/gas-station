-- Copyright (c) 2025 IOTA Stiftung
-- SPDX-License-Identifier: Apache-2.0

-- Release the maintenance lock for the gas station.
-- This is done by setting the lock expiration time to 0.
-- The first argument is the namespace.

local namespace = ARGV[1]

local t_maintenance_lock = namespace .. ':maintenance_lock'
redis.call('SET', t_maintenance_lock, 0)

