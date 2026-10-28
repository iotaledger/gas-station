-- Copyright (c) 2024 IOTA Stiftung
-- SPDX-License-Identifier: Apache-2.0

local namespace = ARGV[1]
local key_name = ARGV[2]
local amount = ARGV[3]
local ttl = tonumber(ARGV[4])

-- we store integers as strings, since Lua otherwise would convert them to double,
-- which can lose precision
local MAX_I64 = '9223372036854775807'
local key = namespace .. ':' .. key_name

if redis.call('EXISTS', key) == 0 then
   redis.call('SET', key, '0', 'EX', ttl)
end

local ok = pcall(redis.call, 'INCRBY', key, amount)
if ok then
  -- redis.call converts output of INCRBY to double, which can lose precision
  -- we call GET to return the underlying integer string directly
  return redis.call('GET', key)
end

-- overflow handling, we clamp the value to 0 or MAX_I64
local sign = amount:sub(1, 1)
local clamped
if sign == '-' then
  clamped = '0'
else
  clamped = MAX_I64
end

redis.call('SET', key, clamped, 'EX', ttl)
return clamped
