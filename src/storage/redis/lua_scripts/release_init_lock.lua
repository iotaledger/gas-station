-- Copyright (c) Mysten Labs, Inc.
-- SPDX-License-Identifier: Apache-2.0

-- Release the lock for initializing a sponsor's account.
-- This is done by setting the lock expiration time to 0.
-- The first argument is the sponsor's address.

local namespace = ARGV[1]

local t_init_lock = namespace .. ':init_lock'
redis.call('SET', t_init_lock, 0)
