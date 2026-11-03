// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, time::Instant};

use anyhow::bail;
use chrono::{Local, NaiveDate};
use tokio::sync::RwLock;

use crate::types::ReservationID;

/// The maximum net gas that can be sponsored in a day.
pub struct GasUsageCap {
    daily_cap: u64,
    reservations: RwLock<Reservations>,
    inner: RwLock<GasUsageCapInner>,
}

struct GasUsageCapInner {
    cur_daily_usage: i64,
    cur_date: NaiveDate,
}

#[derive(Debug, Clone, Default)]
pub struct Reservations {
    /// Map of in-flight reservations and their gas usage.
    usage: BTreeMap<ReservationID, ReservationGasUsage>,
    /// Running total of the amount of gas reserved for execution.
    total: u64,
}

/// A booked amount that can help clearing a reservation.
pub struct Booking {
    amount: u64,
}

impl Reservations {
    /// Adds to the total amount reserved.
    pub fn add_to_total(&mut self, amount: u64) {
        self.total += amount;
    }

    /// Substracts from the total amount reserved.
    pub fn sub_from_total(&mut self, amount: u64) {
        self.total -= amount;
    }

    /// Removes any expired reservations.
    pub fn remove_expired(&mut self, now: Instant) {
        for (_, usage) in self.usage.extract_if(.., |_, usage| usage.expires_at < now) {
            self.total -= usage.amount;
        }
    }
}

#[derive(Copy, Debug, Clone)]
struct ReservationGasUsage {
    /// Maximum amount of gas that can be spent after a reservation.
    amount: u64,
    /// Expiration deadeline.
    expires_at: Instant,
    /// State of the reservation.
    under_execution: bool,
}

impl ReservationGasUsage {
    fn new(amount: u64, expires_at: Instant) -> Self {
        Self {
            amount,
            expires_at,
            under_execution: false,
        }
    }
}

impl GasUsageCap {
    pub fn new(daily_cap: u64) -> Self {
        Self {
            daily_cap,
            reservations: Default::default(),
            inner: RwLock::new(GasUsageCapInner {
                cur_daily_usage: 0,
                cur_date: Local::now().date_naive(),
            }),
        }
    }

    /// Books an amount for subsequent reservation.
    ///
    /// # Errors
    ///
    /// Fails if the requested amount will exceed the daily cap.
    pub async fn book_reservation(&self, amount: u64) -> Result<Booking, anyhow::Error> {
        self.reset_date_maybe().await;
        let mut reservations = self.reservations.write().await;
        reservations.remove_expired(Instant::now());
        let cur_daily_usage = self.inner.read().await.cur_daily_usage;

        let exceeds_cap = reservations
            .total
            .checked_add(amount)
            .is_none_or(|total| total as i128 + cur_daily_usage as i128 > self.daily_cap as i128);
        if exceeds_cap {
            bail!("Gas usage exceeds daily cap");
        }
        reservations.add_to_total(amount);
        Ok(Booking { amount })
    }

    /// Cancells a reservation booking.
    pub async fn cancel_booking(&self, booking: Booking) {
        self.reservations
            .write()
            .await
            .sub_from_total(booking.amount);
    }

    /// Check if the reserved amount exceeds the usage limits.
    pub async fn exceeds_usage(&self, reserved_amount: u64) -> bool {
        self.reset_date_maybe().await;
        let cur_daily_usage = self.inner.read().await.cur_daily_usage;
        cur_daily_usage as u64 + self.reservations.read().await.total + reserved_amount
            >= self.daily_cap
    }

    pub async fn insert_reservation(
        &self,
        reservation_id: ReservationID,
        booking: Booking,
        amount: u64,
        expires_at: Instant,
    ) {
        let mut reservations = self.reservations.write().await;
        reservations
            .usage
            .insert(reservation_id, ReservationGasUsage::new(amount, expires_at));
        reservations.total -= booking.amount;
        reservations.total += amount;
    }

    pub async fn remove_reservation(&self, reservation_id: ReservationID) {
        let mut reservations = self.reservations.write().await;
        if let Some(reservation_usage) = reservations.usage.remove(&reservation_id) {
            reservations.total -= reservation_usage.amount;
        }
    }

    /// Update daily usage and returns the new current usage.
    pub async fn update_usage(&self, usage: i64) -> i64 {
        self.reset_date_maybe().await;
        let mut inner = self.inner.write().await;
        inner.cur_daily_usage += usage;
        inner.cur_daily_usage
    }

    /// Resets the daily gas usage if the day has changed.
    async fn reset_date_maybe(&self) {
        let today = Local::now().date_naive();
        let cur_date = self.inner.read().await.cur_date;
        if cur_date != today {
            self.inner.write().await.reset_date(today);
        }
    }
}

impl GasUsageCapInner {
    fn reset_date(&mut self, new_date: NaiveDate) {
        self.cur_date = new_date;
        self.cur_daily_usage = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn test_gas_usage_cap() {
        let cap = GasUsageCap::new(100);
        assert!(cap.book_reservation(25).await.is_ok());
        cap.update_usage(50).await;
        assert!(cap.book_reservation(20).await.is_ok());
        cap.update_usage(4).await;
        assert!(cap.book_reservation(1).await.is_ok());
        cap.update_usage(1).await;
        assert!(cap.book_reservation(1).await.is_err());
    }

    #[tokio::test]
    async fn test_gas_usage_cap_reset() {
        let today = Local::now().date_naive();
        let cap = GasUsageCap::new(100);
        cap.update_usage(100).await;
        assert!(cap.book_reservation(10).await.is_err());
        cap.inner.write().await.cur_date = today + chrono::Duration::days(1);
        assert!(cap.book_reservation(10).await.is_ok());
    }

    #[tokio::test]
    async fn test_reservations_expire() {
        let cap = GasUsageCap::new(100);
        let booking = cap.book_reservation(100).await.unwrap();
        let reservation_id = 0;
        let expires_at = Instant::now() - Duration::from_secs(1);
        cap.insert_reservation(reservation_id, booking, 100, expires_at)
            .await;
        assert!(
            cap.book_reservation(100).await.is_ok(),
            "we should be able to book as the previous reservation has expired"
        );
    }

    #[tokio::test]
    async fn test_reservations_not_expired() {
        let cap = GasUsageCap::new(100);
        let booking = cap.book_reservation(100).await.unwrap();
        let expires_at = Instant::now() + Duration::from_secs(60);
        cap.insert_reservation(0, booking, 100, expires_at).await;
        assert!(
            cap.book_reservation(1).await.is_err(),
            "the previous reservation has not expired yet"
        );
    }

    #[tokio::test]
    async fn test_remove_reservation() {
        let cap = GasUsageCap::new(100);
        let booking = cap.book_reservation(100).await.unwrap();
        let expires_at = Instant::now() + Duration::from_secs(60);
        cap.insert_reservation(0, booking, 100, expires_at).await;
        cap.remove_reservation(0).await;
        assert!(
            cap.book_reservation(100).await.is_ok(),
            "we should be able to book as the previous reservation was removed"
        );
    }

    #[tokio::test]
    async fn test_cancel_booking() {
        let cap = GasUsageCap::new(100);
        let booking = cap.book_reservation(100).await.unwrap();
        cap.cancel_booking(booking).await;
        assert!(
            cap.book_reservation(100).await.is_ok(),
            "we should be able to book as the previous booking was cancelled"
        );
    }

    #[tokio::test]
    async fn test_negative_usage() {
        let cap = GasUsageCap::new(100);
        cap.update_usage(-50).await;
        assert!(
            cap.book_reservation(150).await.is_ok(),
            "a storage rebate adds to the available amount"
        );
    }

    #[tokio::test]
    async fn test_reserved_total_overflow() {
        let cap = GasUsageCap::new(u64::MAX);
        cap.book_reservation(u64::MAX).await.unwrap();
        assert!(
            cap.book_reservation(1).await.is_err(),
            "a reserved total above u64::MAX should fail the booking"
        );
    }
}
