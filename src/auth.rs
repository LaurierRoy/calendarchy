use crate::google::TokenInfo;

/// Trait for auth state display
pub trait AuthDisplay {
    fn is_authenticated(&self) -> bool;
}

/// Google authentication state
#[derive(Debug, Clone)]
pub enum GoogleAuthState {
    NotConfigured,
    NotAuthenticated,
    Authenticating,
    Authenticated(TokenInfo),
    Error(String),
}

impl AuthDisplay for GoogleAuthState {
    fn is_authenticated(&self) -> bool {
        matches!(self, GoogleAuthState::Authenticated(_))
    }
}

/// Calendar with URL and display name
#[derive(Debug, Clone)]
pub struct CalendarEntry {
    pub url: String,
    pub name: Option<String>,
}

/// iCloud authentication state
#[derive(Debug, Clone)]
pub enum ICloudAuthState {
    NotConfigured,
    NotAuthenticated,
    Discovering,
    Authenticated { calendars: Vec<CalendarEntry> },
    #[allow(dead_code)]
    Error(String),
}

impl AuthDisplay for ICloudAuthState {
    fn is_authenticated(&self) -> bool {
        matches!(self, ICloudAuthState::Authenticated { .. })
    }
}

/// Unified per-account auth state for any provider type
#[derive(Debug, Clone)]
pub enum AccountAuthState {
    NotConfigured,
    Google(GoogleAuthState),
    ICloud(ICloudAuthState),
}

impl AccountAuthState {
    pub fn is_authenticated(&self) -> bool {
        match self {
            Self::Google(g) => g.is_authenticated(),
            Self::ICloud(i) => i.is_authenticated(),
            Self::NotConfigured => false,
        }
    }
}
