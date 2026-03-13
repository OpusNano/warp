#[derive(Debug, Clone)]
pub struct TrustModel;

impl TrustModel {
    pub fn new() -> Self {
        Self
    }

    pub fn host_verification_policy(&self) -> &'static str {
        "strict verify with explicit first-connect trust"
    }
}
