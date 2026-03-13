#[derive(Debug, Clone)]
pub struct ScpCompatibility;

impl ScpCompatibility {
    pub fn new() -> Self {
        Self
    }

    pub fn scope(&self) -> &'static str {
        "transfer-only compatibility mode over russh exec channels"
    }
}
