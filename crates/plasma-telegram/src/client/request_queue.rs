//! Priority scheduler for the single TDLib client.
//!
//! TDLib accepts concurrent requests, but flooding it with chat-list scans,
//! avatars, thumbnails and reply lookups while history is loading freezes the
//! open conversation. This queue keeps Conversation/Critical work ahead of
//! Background/Interactive jobs via leases and a small low-priority slot pool.

use std::future::Future;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Notify;

/// Service class for work that shares the TDLib client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum TdlibPriority {
    /// Soft chat-list scans, avatars, thumbnails, reply previews, sender names.
    Background = 0,
    /// User-driven send / read / jump once the open page is solid.
    Interactive = 1,
    /// Open-chat history (quick local page and network follow-up).
    Conversation = 2,
    /// Auth and client bootstrap.
    Critical = 3,
}

impl TdlibPriority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Interactive => "interactive",
            Self::Conversation => "conversation",
            Self::Critical => "critical",
        }
    }
}

/// Shared scheduler state owned by [`super::TdlibManager`].
pub struct RequestQueue {
    /// Conversation/Critical leases. While greater than zero, low-priority work waits.
    leases: AtomicU32,
    /// Active Background/Interactive jobs.
    low_running: AtomicU32,
    /// Cap concurrent low-priority jobs so a resume stampede cannot bury history.
    low_slots: u32,
    wake: Notify,
}

impl Default for RequestQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestQueue {
    pub fn new() -> Self {
        Self {
            leases: AtomicU32::new(0),
            low_running: AtomicU32::new(0),
            low_slots: 2,
            wake: Notify::new(),
        }
    }

    pub fn conversation_busy(&self) -> bool {
        self.leases.load(Ordering::Acquire) > 0
    }

    pub fn conversation_leases(&self) -> u32 {
        self.leases.load(Ordering::Acquire)
    }

    pub fn low_running(&self) -> u32 {
        self.low_running.load(Ordering::Acquire)
    }

    fn acquire_lease(&self) {
        self.leases.fetch_add(1, Ordering::AcqRel);
    }

    fn release_lease(&self) {
        self.leases.fetch_sub(1, Ordering::AcqRel);
        self.wake.notify_waiters();
    }

    /// Blocks until no Conversation/Critical lease is held (or the timeout).
    pub async fn wait_leases_clear(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if !self.conversation_busy() {
                return;
            }
            if Instant::now() >= deadline {
                return;
            }
            let notified = self.wake.notified();
            if !self.conversation_busy() {
                return;
            }
            tokio::select! {
                _ = notified => {}
                _ = tokio::time::sleep(Duration::from_millis(50)) => {}
            }
        }
    }
}

/// RAII lease that blocks Background/Interactive work while the open chat loads.
pub struct ConversationPriorityGuard {
    queue: Arc<RequestQueue>,
}

impl ConversationPriorityGuard {
    pub(super) fn from_queue(queue: Arc<RequestQueue>) -> Self {
        let leases = queue.leases.fetch_add(1, Ordering::AcqRel) + 1;
        #[cfg(debug_assertions)]
        if leases > 8 {
            tracing::warn!(
                leases,
                low_running = queue.low_running(),
                "TDLib conversation lease depth unusually high"
            );
        }
        Self { queue }
    }
}

impl Drop for ConversationPriorityGuard {
    fn drop(&mut self) {
        self.queue.release_lease();
    }
}

struct LowPermit {
    queue: Arc<RequestQueue>,
}

impl Drop for LowPermit {
    fn drop(&mut self) {
        self.queue.low_running.fetch_sub(1, Ordering::AcqRel);
        self.queue.wake.notify_waiters();
    }
}

struct HighPermit {
    queue: Arc<RequestQueue>,
}

impl Drop for HighPermit {
    fn drop(&mut self) {
        self.queue.release_lease();
    }
}

/// Runs `work` when the queue grants a slot for `priority`.
pub async fn with_priority<F, Fut, T>(
    queue: &Arc<RequestQueue>,
    priority: TdlibPriority,
    label: &'static str,
    work: F,
) -> T
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = T>,
{
    #[cfg(debug_assertions)]
    let enqueued_at = Instant::now();

    let _permit_high: Option<HighPermit>;
    let _permit_low: Option<LowPermit>;
    match priority {
        TdlibPriority::Critical | TdlibPriority::Conversation => {
            queue.acquire_lease();
            _permit_high = Some(HighPermit {
                queue: queue.clone(),
            });
            _permit_low = None;
        }
        TdlibPriority::Interactive | TdlibPriority::Background => {
            _permit_high = None;
            _permit_low = Some(acquire_low_slot(queue, priority, label).await);
        }
    }

    #[cfg(debug_assertions)]
    let started_at = Instant::now();
    #[cfg(debug_assertions)]
    let queue_wait_ms = enqueued_at.elapsed().as_millis() as u64;

    let result = work().await;

    #[cfg(debug_assertions)]
    {
        let work_ms = started_at.elapsed().as_millis() as u64;
        if queue_wait_ms >= 100 || work_ms >= 250 {
            tracing::debug!(
                priority = priority.as_str(),
                label,
                queue_wait_ms,
                work_ms,
                leases = queue.conversation_leases(),
                low_running = queue.low_running(),
                "TDLib queue job finished"
            );
        }
    }

    // Keep permits alive through the work future.
    drop(_permit_high);
    drop(_permit_low);

    result
}

async fn acquire_low_slot(
    queue: &Arc<RequestQueue>,
    priority: TdlibPriority,
    label: &'static str,
) -> LowPermit {
    #[cfg(debug_assertions)]
    let started = Instant::now();
    loop {
        if !queue.conversation_busy() {
            let current = queue.low_running.load(Ordering::Acquire);
            if current < queue.low_slots
                && queue
                    .low_running
                    .compare_exchange(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                if queue.conversation_busy() {
                    queue.low_running.fetch_sub(1, Ordering::AcqRel);
                    queue.wake.notify_waiters();
                } else {
                    #[cfg(debug_assertions)]
                    {
                        let waited_ms = started.elapsed().as_millis() as u64;
                        if waited_ms >= 100 {
                            tracing::debug!(
                                priority = priority.as_str(),
                                label,
                                waited_ms,
                                leases = queue.conversation_leases(),
                                low_running = queue.low_running(),
                                "TDLib queue granted low-priority slot"
                            );
                        }
                    }
                    let _ = (priority, label);
                    return LowPermit {
                        queue: queue.clone(),
                    };
                }
            }
        }

        let notified = queue.wake.notified();
        if !queue.conversation_busy() && queue.low_running.load(Ordering::Acquire) < queue.low_slots
        {
            continue;
        }
        tokio::select! {
            _ = notified => {}
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    }
}
