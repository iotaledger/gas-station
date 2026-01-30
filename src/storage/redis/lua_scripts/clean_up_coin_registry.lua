-- Copyright (c) Mysten Labs, Inc.
-- SPDX-License-Identifier: Apache-2.0

-- This script is used to clean up all data associated with a sponsor's coin registry.
-- It deletes all keys with the namespace prefix.
-- The first argument is the namespace.

local namespace = ARGV[1]
local pattern = namespace .. ':*'

local cursor = "0"
local deleted_count = 0

repeat
    local result = redis.call('SCAN', cursor, 'MATCH', pattern, 'COUNT', 1000)
    cursor = result[1]
    local keys = result[2]

    if #keys > 0 then
        redis.call('DEL', unpack(keys))
        deleted_count = deleted_count + #keys
    end
until cursor == "0"

return deleted_count

