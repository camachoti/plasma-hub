//! UI-agnostic application events.
//!
//! Domain crates publish [`AppEvent`] values on an [`EventBus`]; the UI layer
//! subscribes and maps them to its own signals. The Tauri bridge still uses
//! the legacy named events until the cutover, so the two mechanisms can
//! coexist during the migration.

use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;

/// Events consumed by the Rust UI.
///
/// Payloads are deliberately simple (no domain structs) so this crate stays at
/// the bottom of the dependency graph. High-frequency events are coalesced by
/// the producer, not by the bus.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum AppEvent {
    TelegramAuth {
        state: String,
    },
    TelegramNewMessage {
        chat_id: i64,
        topic_id: Option<i64>,
        message: Value,
    },
    TelegramMessagesDeleted {
        chat_id: i64,
        message_ids: Vec<i64>,
    },
    MediaProgress {
        chat_id: i64,
        message_id: i64,
        downloaded_bytes: u64,
        total_bytes: u64,
        state: String,
    },
    DownloadProgress {
        id: String,
        downloaded_bytes: u64,
        total_bytes: u64,
        /// Optional percentage when the producer only knows progress.
        percent: Option<f64>,
    },
    DownloadDone {
        id: String,
    },
    DownloadError {
        id: String,
        error: String,
    },
    SharedUrl {
        url: String,
    },
    SearchProgress {
        run_id: String,
        scanned: u64,
        total: u64,
    },
    Log {
        level: String,
        message: String,
    },
}

impl AppEvent {
    /// Stable event name, useful for logging and for the legacy bridge.
    pub fn name(&self) -> &'static str {
        match self {
            Self::TelegramAuth { .. } => "telegram-auth",
            Self::TelegramNewMessage { .. } => "telegram-new-message",
            Self::TelegramMessagesDeleted { .. } => "telegram-messages-deleted",
            Self::MediaProgress { .. } => "media-progress",
            Self::DownloadProgress { .. } => "download-progress",
            Self::DownloadDone { .. } => "download-done",
            Self::DownloadError { .. } => "download-error",
            Self::SharedUrl { .. } => "shared-url",
            Self::SearchProgress { .. } => "search-progress",
            Self::Log { .. } => "log",
        }
    }

    pub fn payload(&self) -> crate::Result<Value> {
        serde_json::to_value(self).map_err(Into::into)
    }
}

/// Multi-consumer event bus used by the Rust app shell.
#[derive(Clone, Debug)]
pub struct EventBus {
    sender: broadcast::Sender<AppEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _receiver) = broadcast::channel(capacity.max(1));
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.sender.subscribe()
    }

    /// Publishes an event. Returns the number of active receivers.
    ///
    /// A publish with no subscribers is not an error: the app may emit events
    /// before the UI has mounted, and those events are deliberately dropped.
    pub fn publish(&self, event: AppEvent) -> usize {
        self.sender.send(event).unwrap_or(0)
    }

    pub fn receiver_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn broadcasts_events_to_subscribers() {
        let bus = EventBus::new(8);
        let mut receiver = bus.subscribe();
        assert_eq!(bus.receiver_count(), 1);

        let sent = bus.publish(AppEvent::DownloadDone {
            id: "job-1".to_string(),
        });
        assert_eq!(sent, 1);

        let event = receiver.try_recv().expect("event");
        assert_eq!(event.name(), "download-done");
        assert_eq!(
            event.payload().expect("payload"),
            json!({
                "type": "downloadDone",
                "id": "job-1"
            })
        );
    }

    #[test]
    fn serializes_shared_url_events() {
        let event = AppEvent::SharedUrl {
            url: "https://youtu.be/abc".to_string(),
        };
        assert_eq!(event.name(), "shared-url");
        assert_eq!(
            event.payload().expect("payload"),
            json!({
                "type": "sharedUrl",
                "url": "https://youtu.be/abc"
            })
        );
    }

    #[test]
    fn publishing_without_subscribers_is_not_an_error() {
        let bus = EventBus::new(1);
        assert_eq!(
            bus.publish(AppEvent::Log {
                level: "info".to_string(),
                message: "hello".to_string(),
            }),
            0
        );
    }

    #[test]
    fn lagging_receivers_keep_the_latest_events_available() {
        let bus = EventBus::new(2);
        let mut receiver = bus.subscribe();
        for id in 0..5 {
            bus.publish(AppEvent::DownloadProgress {
                id: format!("job-{id}"),
                downloaded_bytes: id,
                total_bytes: 10,
                percent: None,
            });
        }

        // The receiver has no capacity to retain every event; it must recover
        // with a `Lagged` error and continue reading the newest ones.
        assert!(matches!(
            receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(_))
        ));
    }
}
