#[derive(Debug, Clone)]
pub struct RemoteSftpEngine;

impl RemoteSftpEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn protocol_foundation(&self) -> &'static str {
        "russh-sftp over russh session channel"
    }
}
