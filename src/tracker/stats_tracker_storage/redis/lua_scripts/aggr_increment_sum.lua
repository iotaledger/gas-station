-- Copyright (c) 2024 IOTA Stiftung
-- SPDX-License-Identifier: Apache-2.0

local sponsor_address = ARGV[1]
local key_name = ARGV[2]
local amount = tonumber(ARGV[3])
local ttl = tonumber(ARGV[4])


local MAX_I64 = 9223372036854775807
local key = sponsor_address .. ':' .. key_name

if redis.call('EXISTS', key) == 0 then
   redis.call('SET', key, '0', 'EX', ttl)
end

local ok, new_val = pcall(redis.call, 'INCRBY', key, amount)
if ok then
  return new_val
end

-- overflow handling
redis.call('SET', key, MAX_I64)
return MAX_I64


