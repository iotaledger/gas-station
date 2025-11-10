// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use tokio::sync::mpsc::{error::TrySendError, Receiver, Sender};
use tracing::{error, warn};

/// The buffer size for the rescan trigger channel.
/// It is set to 5 to limit how frequently rescans can be triggered.
/// Additional rescan requests beyond this limit will be discarded if the buffer is full.
const RESCAN_TRIGGER_CHANNEL_BUFFER_SIZE: usize = 5;

#[derive(Clone)]
pub struct RescanGasObjectsTrigger {
    pub target_init_balance: u64,
    trigger_rescan_sender: Option<Sender<()>>,
}

impl RescanGasObjectsTrigger {
    /// Create a new RescanGasObjectsTrigger with the given target initial balance.
    /// Please make sure the channel is set before triggering rescan by calling
    /// [`with_trigger_rescan_sender`](Self::with_trigger_rescan_sender) or [`create_receiver`](Self::create_receiver).
    pub fn new(target_init_balance: u64) -> Self {
        Self {
            target_init_balance,
            trigger_rescan_sender: None,
        }
    }

    pub fn with_trigger_rescan_sender(mut self, trigger_rescan_sender: Sender<()>) -> Self {
        self.trigger_rescan_sender = Some(trigger_rescan_sender);
        self
    }

    /// Create a new receiver for the rescan trigger.
    pub fn create_receiver(&mut self) -> Receiver<()> {
        let (sender, receiver) =
            tokio::sync::mpsc::channel::<()>(RESCAN_TRIGGER_CHANNEL_BUFFER_SIZE);
        self.trigger_rescan_sender = Some(sender);
        receiver
    }

    /// Trigger a rescan for new coins.
    /// This method will send a message to the rescan trigger channel.
    /// If the channel is full, the request will be dropped.
    pub async fn trigger_rescan(&self) {
        if let Some(trigger_rescan_sender) = self.trigger_rescan_sender.as_ref() {
            match trigger_rescan_sender.try_send(()) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    warn!("Failed to send rescan trigger. Too many requests are pending. Dropping the request.");
                }
                Err(TrySendError::Closed(_)) => {
                    error!("Failed to send rescan trigger. The channel has been closed. Please restart the Gas Station.");
                }
            }
        } else {
            warn!("No trigger rescan sender found. Please set it up before triggering rescan");
        }
    }
}
