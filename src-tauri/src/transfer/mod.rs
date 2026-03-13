#[derive(Debug, Clone)]
pub struct TransferEngine;

impl TransferEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn queue_model(&self) -> &'static str {
        "bounded queue with rust-owned progress events"
    }
}
