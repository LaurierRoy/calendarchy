use crate::config::ICloudConfig;
use base64::{engine::general_purpose::STANDARD, Engine};

/// iCloud authentication helper
pub struct ICloudAuth {
    config: ICloudConfig,
}

impl ICloudAuth {
    pub fn new(config: ICloudConfig) -> Self {
        Self { config }
    }

    /// Generate Basic auth header value
    pub fn auth_header(&self) -> String {
        let apple_id = self.config.apple_id.as_deref().unwrap_or("");
        let app_password = self.config.app_password.as_deref().unwrap_or("");
        let credentials = format!("{}:{}", apple_id, app_password);
        let encoded = STANDARD.encode(credentials.as_bytes());
        format!("Basic {}", encoded)
    }

}
