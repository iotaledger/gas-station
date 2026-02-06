-- Copyright (c) Mysten Labs, Inc.
-- SPDX-License-Identifier: Apache-2.0

-- This script is used to clean up all data associated with a sponsor's coin registry.
-- It deletes all keys with the namespace prefix, EXCEPT for lock keys which must be preserved.
-- The first argument is the namespace.

local namespace = ARGV[1]
local pattern = namespace .. ':*'

-- Lock keys that should NOT be deleted during cleanup
local t_maintenance_lock = namespace .. ':maintenance_lock'
local t_init_lock = namespace .. ':init_lock'

local cursor = "0"
local deleted_count = 0

repeat
    local result = redis.call('SCAN', cursor, 'MATCH', pattern, 'COUNT', 1000)
    cursor = result[1]
    local keys = result[2]

    if #keys > 0 then
        -- Filter out lock keys that should be preserved
        local keys_to_delete = {}
        for i, key in ipairs(keys) do
            if key ~= t_maintenance_lock and key ~= t_init_lock then
                table.insert(keys_to_delete, key)
            end
        end
        
        if #keys_to_delete > 0 then
            redis.call('DEL', unpack(keys_to_delete))
            deleted_count = deleted_count + #keys_to_delete
        end
    end
until cursor == "0"

return deleted_count

