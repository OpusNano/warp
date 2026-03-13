#[derive(Debug, Clone)]
pub struct SessionEngine;

impl SessionEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn protocol_foundation(&self) -> &'static str {
        "russh transport + auth + channels"
    }
}
