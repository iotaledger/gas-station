use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::ValueNumber;

/// ValueAggregate is a struct that represents an aggregate value with a specified window and limit.
/// It must use persistent storage [`Tracker`] to store the aggregate value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueAggregate {
    pub window: Duration,
    pub limit: ValueNumber,
}
