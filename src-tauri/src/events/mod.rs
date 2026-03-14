#[derive(Debug, Clone)]
pub struct UiEvents;

pub const TRANSFER_QUEUE_UPDATED_EVENT: &str = "transfer-queue-updated";
pub const REMOTE_SESSION_UPDATED_EVENT: &str = "remote-session-updated";

impl UiEvents {
    pub fn new() -> Self {
        Self
    }
}
