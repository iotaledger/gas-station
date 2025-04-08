-- Copyright (c) 2024 IOTA Stiftung
-- SPDX-License-Identifier: Apache-2.0

local sponsor_address = ARGV[1]
local key_name = ARGV[2]
local amount = tonumber(ARGV[3])
local ttl = tonumber(ARGV[4])


local key = sponsor_address .. ':' .. key_name

if redis.call('EXISTS', key) == 0 then
    redis.call('SET', key, 0)
    redis.call('EXPIRE', key, ttl)
end

local new_value =  redis.call('INCRBY', key, amount)
return new_value


