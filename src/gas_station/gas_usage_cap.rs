// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeSet, HashMap},
    time::Instant,
};

use anyhow::bail;
use chrono::{Local, NaiveDate};
use tokio::sync::{Mutex, MutexGuard};

use crate::types::ReservationID;

/// The maximum net gas that can be sponsored in a day.
pub struct GasUsageCap {
    daily_cap: u64,
    usage: Mutex<Usage>,
}

/// The daily gas usage and the gas reserved for execution.
pub struct Usage {
    reservations: Reservations,
    cur_daily_usage: i64,
    cur_date: NaiveDate,
}

#[derive(Debug, Default)]
struct Reservations {
    /// Map of in-flight reservations and their gas usage.
    inner: HashMap<ReservationID, Reservation>,
    /// The expiration time of each reservation, in the order of expiration.
    expirations: BTreeSet<(Instant, ReservationID)>,
    /// Running total of the amount of gas reserved for execution.
    total: u64,
}

#[derive(Copy, Debug, Clone)]
struct Reservation {
    /// Maximum amount of gas that can be spent after a reservation.
    amount: u64,
    /// Number of active execution requests.
    active_execution_requests: usize,
}

/// The amount booked until a reservation is resolved from storage.
pub struct Booking {
    amount: u64,
}

impl GasUsageCap {
    pub fn new(daily_cap: u64) -> Self {
        Self {
            daily_cap,
            usage: Mutex::new(Usage {
                reservations: Default::default(),
                cur_daily_usage: 0,
                cur_date: Local::now().date_naive(),
            }),
        }
    }

    /// Reserves a write lock on the inner usage.
    ///
    /// Must be released as soon as possible to avoid contention and deadlocks.
    pub async fn usage_lock(&self) -> MutexGuard<'_, Usage> {
        self.usage.lock().await
    }

    /// Increases the inner counter of active requests for the given reservation.
    pub async fn add_execution_request(&self, reservation_id: ReservationID) {
        self.usage_lock()
            .await
            .reservations
            .add_execution_request(reservation_id)
    }

    /// Decreases the counter of active requests for the given reservation.
    pub async fn remove_execution_request(&self, reservation_id: ReservationID) {
        self.usage_lock()
            .await
            .reservations
            .remove_execution_request(reservation_id)
    }

    /// Books an amount for subsequent reservation.
    ///
    /// # Errors
    ///
    /// Fails if the requested amount will exceed the daily cap.
    pub async fn book_reservation(&self, amount: u64) -> anyhow::Result<Booking> {
        self.usage_lock().await.book(amount, self.daily_cap)
    }

    /// Cancels a reservation booking.
    pub async fn cancel_booking(&self, booking: Booking) {
        self.usage_lock().await.reservations.total -= booking.amount
    }

    /// Replaces the booking with a reservation of the given amount.
    pub async fn insert_reservation(
        &self,
        reservation_id: ReservationID,
        booking: Booking,
        amount: u64,
        expires_at: Instant,
    ) {
        self.usage_lock()
            .await
            .reservations
            .insert(reservation_id, booking, amount, expires_at)
    }

    /// Removes the given reservation, and charges nothing.
    pub async fn remove_reservation(&self, reservation_id: ReservationID) {
        self.usage_lock().await.remove_reservation(reservation_id);
    }
}

impl Usage {
    /// Adds to the daily usage and returns the new current usage.
    pub fn add(&mut self, usage: i64) -> i64 {
        self.reset_date_maybe();
        self.cur_daily_usage = self.cur_daily_usage.saturating_add(usage);
        self.cur_daily_usage
    }

    /// Removes a reservation if it exists, and returns its amount.
    pub fn remove_reservation(&mut self, reservation_id: ReservationID) -> Option<u64> {
        self.reservations.remove(reservation_id)
    }

    /// Books `amount` if the daily usage and the reserved total stay within `daily_cap`.
    fn book(&mut self, amount: u64, daily_cap: u64) -> anyhow::Result<Booking> {
        self.reset_date_maybe();
        self.reservations.remove_expired(Instant::now());

        let exceeds_cap = self
            .reservations
            .total
            .checked_add(amount)
            .is_none_or(|total| total as i128 + self.cur_daily_usage as i128 > daily_cap as i128);
        if exceeds_cap {
            bail!("Gas usage exceeds daily cap");
        }
        self.reservations.total += amount;
        Ok(Booking { amount })
    }

    /// Resets the daily gas usage if the day has changed.
    fn reset_date_maybe(&mut self) {
        let today = Local::now().date_naive();
        if self.cur_date != today {
            self.cur_date = today;
            self.cur_daily_usage = 0;
        }
    }
}

impl Reservations {
    /// Removes the reservations that expired at or before `now`.
    ///
    /// A reservation with an active execution request stays, and the next call
    /// checks it again.
    fn remove_expired(&mut self, now: Instant) {
        let inner = &self.inner;
        let expired: Vec<ReservationID> = self
            .expirations
            .extract_if(..=(now, ReservationID::MAX), |(_, reservation_id)| {
                // `None`: the reservation was removed before it expired.
                inner
                    .get(reservation_id)
                    .is_none_or(|reservation| reservation.active_execution_requests == 0)
            })
            .map(|(_, reservation_id)| reservation_id)
            .collect();
        for reservation_id in expired {
            self.remove(reservation_id);
        }
    }

    /// Replaces a booking with a reservation of the given amount.
    fn insert(
        &mut self,
        reservation_id: ReservationID,
        booking: Booking,
        amount: u64,
        expires_at: Instant,
    ) {
        self.inner.insert(reservation_id, Reservation::new(amount));
        self.expirations.insert((expires_at, reservation_id));
        self.total -= booking.amount;
        self.total += amount;
    }

    fn add_execution_request(&mut self, reservation_id: ReservationID) {
        if let Some(reservation) = self.inner.get_mut(&reservation_id) {
            reservation.active_execution_requests += 1;
        }
    }

    fn remove_execution_request(&mut self, reservation_id: ReservationID) {
        if let Some(reservation) = self.inner.get_mut(&reservation_id) {
            reservation.active_execution_requests =
                reservation.active_execution_requests.saturating_sub(1);
        }
    }

    /// Removes a reservation if it exists, and returns its amount.
    fn remove(&mut self, reservation_id: ReservationID) -> Option<u64> {
        let reservation = self.inner.remove(&reservation_id)?;
        self.total -= reservation.amount;
        Some(reservation.amount)
    }
}

impl Reservation {
    fn new(amount: u64) -> Self {
        Self {
            amount,
            active_execution_requests: 0,
        }
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
        cap.usage_lock().await.add(50);
        assert!(cap.book_reservation(20).await.is_ok());
        cap.usage_lock().await.add(4);
        assert!(cap.book_reservation(1).await.is_ok());
        cap.usage_lock().await.add(1);
        assert!(cap.book_reservation(1).await.is_err());
    }

    #[tokio::test]
    async fn test_gas_usage_cap_reset() {
        let today = Local::now().date_naive();
        let cap = GasUsageCap::new(100);
        cap.usage_lock().await.add(100);
        assert!(cap.book_reservation(10).await.is_err());
        cap.usage_lock().await.cur_date = today + chrono::Duration::days(1);
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
    async fn test_reservations_ready_for_execution_not_expired() {
        let cap = GasUsageCap::new(100);
        let booking = cap.book_reservation(100).await.unwrap();
        let expires_at = Instant::now() - Duration::from_secs(60);
        cap.insert_reservation(0, booking, 100, expires_at).await;
        cap.add_execution_request(0).await;
        assert!(
            cap.book_reservation(1).await.is_err(),
            "the previous reservation is ready for execution so not expired yet"
        );
    }

    #[tokio::test]
    async fn test_reservation_expired_during_execution() {
        let cap = GasUsageCap::new(100);
        let booking = cap.book_reservation(100).await.unwrap();
        let expires_at = Instant::now() - Duration::from_secs(60);
        cap.insert_reservation(0, booking, 100, expires_at).await;
        cap.add_execution_request(0).await;
        assert!(cap.book_reservation(1).await.is_err());
        cap.remove_execution_request(0).await;
        assert!(
            cap.book_reservation(100).await.is_ok(),
            "the next booking should remove the expired reservation when no request is active"
        );
    }

    #[tokio::test]
    async fn test_removed_reservation_expires_without_effect() {
        let cap = GasUsageCap::new(100);
        let booking = cap.book_reservation(60).await.unwrap();
        let expires_at = Instant::now() - Duration::from_secs(1);
        cap.insert_reservation(0, booking, 60, expires_at).await;
        cap.remove_reservation(0).await;
        let _booking = cap.book_reservation(100).await.unwrap();
        let usage = cap.usage_lock().await;
        assert!(usage.reservations.expirations.is_empty());
        assert_eq!(
            usage.reservations.total, 100,
            "the expiration of a removed reservation should not change the total"
        );
    }

    #[tokio::test]
    async fn test_reservations_with_equal_expiration() {
        let cap = GasUsageCap::new(100);
        let expires_at = Instant::now() - Duration::from_secs(1);
        for reservation_id in 0..2 {
            let booking = cap.book_reservation(50).await.unwrap();
            cap.insert_reservation(reservation_id, booking, 50, expires_at)
                .await;
        }
        assert!(
            cap.book_reservation(100).await.is_ok(),
            "both reservations should expire"
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
        cap.usage_lock().await.add(-50);
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
