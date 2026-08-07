-- Copyright (c) Mysten Labs, Inc.
-- Modifications Copyright (c) 2026 IOTA Stiftung
-- SPDX-License-Identifier: Apache-2.0

-- This script is used to mark a reservation as ready for execution.
-- It takes out the reservation from the sponsor's reservation map.
-- We need this such that a concurrent task that calls expire_coins.lua does not expire the same reservation again
-- right before the transaction is executed.
--
-- Before consuming it, verifies the caller is paying with exactly the coins this
-- reservation owns: a reservation id alone is not proof of ownership.
--
-- The comparison MUST stay in this script, before the DEL. It has to be atomic
-- with the delete, and Redis does not roll back writes made before an error.
--
-- The first argument is the sponsor's namespace.
-- The second argument is the reservation id.
-- The third argument is the comma-separated object ids of the transaction's gas
-- payment, in the same textual form reserve_gas_coins.lua stored them in.

local namespace = ARGV[1]
local reservation_id = ARGV[2]
local payment_object_ids = ARGV[3]

local key = namespace .. ':' .. reservation_id
local reserved_object_ids = redis.call('GET', key)
if reserved_object_ids == false then
    error('Reservation no longer exist: ' .. reservation_id)
end

local reserved = {}
local reserved_count = 0
for object_id in string.gmatch(reserved_object_ids, '([^,]+)') do
    if reserved[object_id] == nil then
        reserved[object_id] = true
        reserved_count = reserved_count + 1
    end
end

-- Exact set equality. A subset is rejected too: the DEL is wholesale, so coins
-- left out would be stranded. Errors report counts only, never the reserved ids.
local payment_count = 0
for object_id in string.gmatch(payment_object_ids, '([^,]+)') do
    if reserved[object_id] == nil then
        error('Gas coins in the transaction do not belong to reservation ' .. reservation_id)
    end
    -- Consume it, so a repeated coin cannot stand in for a missing one.
    reserved[object_id] = nil
    payment_count = payment_count + 1
end

if payment_count ~= reserved_count then
    error('Transaction pays with ' .. payment_count .. ' gas coins but reservation '
        .. reservation_id .. ' holds ' .. reserved_count)
end

redis.call('DEL', key)
