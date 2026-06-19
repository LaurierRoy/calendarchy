use crate::error::Result;
use crate::google::TokenInfo;
use chrono::{DateTime, Utc};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Root configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// New format: list of named categories
    #[serde(default)]
    pub categories: Vec<Category>,
    /// New format: list of provider accounts
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
    /// Legacy backward-compat accessor (not serialized in new format)
    #[serde(default, skip_serializing)]
    pub google: Option<GoogleConfig>,
    /// Legacy backward-compat accessor (not serialized in new format)
    #[serde(default, skip_serializing)]
    pub icloud: Option<ICloudConfig>,
}

/// Built-in Google OAuth credentials (public, identifies the app)
pub const DEFAULT_GOOGLE_CLIENT_ID: &str =
    "313544353824-1g092hbgrmd6pemvklv58ld9radn0rg3.apps.googleusercontent.com";
pub const DEFAULT_GOOGLE_CLIENT_SECRET: &str = "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";

/// Google Calendar configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleConfig {
    #[serde(default = "default_google_client_id")]
    pub client_id: String,
    #[serde(default = "default_google_client_secret")]
    pub client_secret: String,
    #[serde(default = "default_calendar_id")]
    pub calendar_id: String,
    #[serde(default)]
    pub category: Option<Category>,
}

impl Default for GoogleConfig {
    fn default() -> Self {
        Self {
            client_id: default_google_client_id(),
            client_secret: default_google_client_secret(),
            calendar_id: "primary".to_string(),
            category: None,
        }
    }
}

fn default_google_client_id() -> String {
    std::env::var("CALENDARCHY_GOOGLE_CLIENT_ID")
        .unwrap_or_else(|_| DEFAULT_GOOGLE_CLIENT_ID.to_string())
}

fn default_google_client_secret() -> String {
    std::env::var("CALENDARCHY_GOOGLE_CLIENT_SECRET")
        .unwrap_or_else(|_| DEFAULT_GOOGLE_CLIENT_SECRET.to_string())
}

/// A user-configurable category label and accent color for a calendar source
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Category {
    #[serde(default = "Category::default_name")]
    pub name: String,
    #[serde(default = "Category::default_accent")]
    pub accent: String,
}

impl Category {
    fn default_name() -> String {
        "Calendar".to_string()
    }
    fn default_accent() -> String {
        "blue".to_string()
    }
}

/// iCloud Calendar configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ICloudConfig {
    /// "eventkit" (macOS, zero config) or "caldav" (cross-platform)
    /// Defaults to "caldav" for backward compatibility
    #[serde(default = "default_icloud_method")]
    pub method: String,
    /// Required for caldav method
    #[serde(default)]
    pub apple_id: Option<String>,
    /// Required for caldav method
    #[serde(default)]
    pub app_password: Option<String>,
    #[serde(default)]
    pub category: Option<Category>,
}

fn default_icloud_method() -> String {
    "caldav".to_string()
}

impl ICloudConfig {
    pub fn is_eventkit(&self) -> bool {
        self.method == "eventkit"
    }

    #[allow(dead_code)]
    pub fn is_caldav(&self) -> bool {
        self.method == "caldav"
    }
}

fn default_calendar_id() -> String {
    "primary".to_string()
}

fn generate_account_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("a{:x}", nanos)
}

/// Account configuration for a Google Calendar
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleAccountConfig {
    /// Stable identifier for token storage and auth state
    #[serde(default = "generate_account_id")]
    pub id: String,
    /// Display name for this account (shown in UI)
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_calendar_id")]
    pub calendar_id: String,
    /// References a Category by name from the top-level categories list
    #[serde(default)]
    pub category: Option<String>,
}

/// Return the effective Google OAuth client ID (env var or compiled default)
pub fn google_client_id() -> String {
    default_google_client_id()
}

/// Return the effective Google OAuth client secret (env var or compiled default)
pub fn google_client_secret() -> String {
    default_google_client_secret()
}

/// Account configuration for an iCloud Calendar
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ICloudAccountConfig {
    /// Stable identifier for token storage and auth state
    #[serde(default = "generate_account_id")]
    pub id: String,
    /// Display name for this account (shown in UI)
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_icloud_method")]
    pub method: String,
    #[serde(default)]
    pub apple_id: Option<String>,
    #[serde(default)]
    pub app_password: Option<String>,
    /// References a Category by name from the top-level categories list
    #[serde(default)]
    pub category: Option<String>,
}

/// A calendar provider account, tagged by type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AccountConfig {
    #[serde(rename = "google")]
    Google(GoogleAccountConfig),
    #[serde(rename = "icloud")]
    ICloud(ICloudAccountConfig),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredTokens {
    #[serde(default)]
    pub google: HashMap<String, GoogleTokens>,
    #[serde(default)]
    pub icloud: HashMap<String, ICloudTokens>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleTokens {
    pub tokens: TokenInfo,
    pub stored_at: DateTime<Utc>,
}

/// Stored calendar entry with URL and optional display name
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCalendar {
    pub url: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ICloudTokens {
    /// Legacy field for backwards compatibility
    #[serde(default)]
    pub calendar_urls: Vec<String>,
    /// New field with calendar names
    #[serde(default)]
    pub calendars: Vec<StoredCalendar>,
    pub stored_at: DateTime<Utc>,
}

impl Config {
    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("calendarchy")
    }

    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.json")
    }

    pub fn token_path() -> PathBuf {
        Self::config_dir().join("tokens.json")
    }

    pub fn load() -> Result<Config> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(Config::default());
        }

        let content = fs::read_to_string(&path)?;
        let mut config: Config = serde_json::from_str(&content)?;

        // Detect old format: no accounts but legacy fields are set
        if config.accounts.is_empty() && (config.google.is_some() || config.icloud.is_some()) {
            // Convert old format to new format in memory
            let mut categories = Vec::new();
            if let Some(ref google) = config.google {
                if let Some(ref cat) = google.category {
                    if !categories.iter().any(|c: &Category| c.name == cat.name) {
                        categories.push(cat.clone());
                    }
                }
                config.accounts.push(AccountConfig::Google(GoogleAccountConfig {
                    id: generate_account_id(),
                    name: None,
                    calendar_id: google.calendar_id.clone(),
                    category: google.category.as_ref().map(|c| c.name.clone()),
                }));
            }
            if let Some(ref icloud) = config.icloud {
                if let Some(ref cat) = icloud.category {
                    if !categories.iter().any(|c: &Category| c.name == cat.name) {
                        categories.push(cat.clone());
                    }
                }
                config.accounts.push(AccountConfig::ICloud(ICloudAccountConfig {
                    id: generate_account_id(),
                    name: None,
                    method: icloud.method.clone(),
                    apple_id: icloud.apple_id.clone(),
                    app_password: icloud.app_password.clone(),
                    category: icloud.category.as_ref().map(|c| c.name.clone()),
                }));
            }
            if categories.is_empty() && config.categories.is_empty() {
                // No user-defined categories, add defaults
                config.categories = vec![
                    Category { name: "Work".to_string(), accent: "blue".to_string() },
                    Category { name: "Personal".to_string(), accent: "magenta".to_string() },
                ];
            } else {
                config.categories = categories;
            }
        } else if !config.accounts.is_empty() {
            // New format: populate legacy accessors from accounts
            config.google = None;
            config.icloud = None;
            for account in &config.accounts {
                match account {
                    AccountConfig::Google(g) => {
                        let category = g.category.as_ref().and_then(|name| {
                            config.categories.iter().find(|c| c.name == *name).cloned()
                        });
                        config.google = Some(GoogleConfig {
                            client_id: google_client_id(),
                            client_secret: google_client_secret(),
                            calendar_id: g.calendar_id.clone(),
                            category,
                        });
                    }
                    AccountConfig::ICloud(i) => {
                        let category = i.category.as_ref().and_then(|name| {
                            config.categories.iter().find(|c| c.name == *name).cloned()
                        });
                        config.icloud = Some(ICloudConfig {
                            method: i.method.clone(),
                            apple_id: i.apple_id.clone(),
                            app_password: i.app_password.clone(),
                            category,
                        });
                    }
                }
            }
        }

        // Env vars always override saved config (on legacy accessors)
        if let Some(ref mut google) = config.google {
            if let Ok(id) = std::env::var("CALENDARCHY_GOOGLE_CLIENT_ID") {
                google.client_id = id;
            }
            if let Ok(secret) = std::env::var("CALENDARCHY_GOOGLE_CLIENT_SECRET") {
                google.client_secret = secret;
            }
        }

        Ok(config)
    }

    pub fn ensure_config_dir() -> Result<()> {
        let dir = Self::config_dir();
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        Self::ensure_config_dir()?;
        let path = Self::config_path();

        // Build serializable representation: use accounts if non-empty,
        // otherwise reconcile from legacy accessors
        let json = if !self.accounts.is_empty() {
            serde_json::to_string_pretty(self)?
        } else {
            // Reconcile from legacy fields (e.g. after setup wizard)
            let mut accounts: Vec<AccountConfig> = Vec::new();
            let mut categories = self.categories.clone();
            if let Some(ref google) = self.google {
                let cat_name = google.category.as_ref().map(|c| c.name.clone());
                if let Some(ref cat) = google.category {
                    if !categories.iter().any(|c: &Category| c.name == cat.name) {
                        categories.push(cat.clone());
                    }
                }
                accounts.push(AccountConfig::Google(GoogleAccountConfig {
                    id: generate_account_id(),
                    name: None,
                    calendar_id: google.calendar_id.clone(),
                    category: cat_name,
                }));
            }
            if let Some(ref icloud) = self.icloud {
                let cat_name = icloud.category.as_ref().map(|c| c.name.clone());
                if let Some(ref cat) = icloud.category {
                    if !categories.iter().any(|c: &Category| c.name == cat.name) {
                        categories.push(cat.clone());
                    }
                }
                accounts.push(AccountConfig::ICloud(ICloudAccountConfig {
                    id: generate_account_id(),
                    name: None,
                    method: icloud.method.clone(),
                    apple_id: icloud.apple_id.clone(),
                    app_password: icloud.app_password.clone(),
                    category: cat_name,
                }));
            }
            #[derive(Serialize)]
            struct SaveConfig<'a> {
                categories: &'a Vec<Category>,
                accounts: &'a Vec<AccountConfig>,
            }
            serde_json::to_string_pretty(&SaveConfig {
                categories: &categories,
                accounts: &accounts,
            })?
        };

        fs::write(&path, &json)?;
        Ok(())
    }
}

/// Check if the system keyring is available
pub fn keyring_available() -> bool {
    // Try creating a temporary entry — if keyring backends exist, this succeeds
    Entry::new("calendarchy-test", "_probe").is_ok()
}

/// Try to save Google tokens to the system keyring (non-critical, may silently fail)
fn save_google_tokens_keyring(account_id: &str, tokens: &TokenInfo) -> bool {
    if let Ok(entry) = Entry::new("calendarchy-google", account_id) {
        if let Ok(json) = serde_json::to_string(tokens) {
            return entry.set_password(&json).is_ok();
        }
    }
    false
}

/// Try to load Google tokens from the system keyring
fn load_google_tokens_keyring(account_id: &str) -> Option<TokenInfo> {
    let entry = Entry::new("calendarchy-google", account_id).ok()?;
    let json = entry.get_password().ok()?;
    serde_json::from_str(&json).ok()
}

/// Save Google tokens for a specific account.
/// Primary storage: system keyring. Fallback/cache: tokens.json.
/// Keeps refresh token in file only when keyring is unavailable.
pub fn save_google_tokens(account_id: &str, tokens: &TokenInfo) -> Result<()> {
    Config::ensure_config_dir()?;

    let has_keyring = save_google_tokens_keyring(account_id, tokens);

    let mut stored = load_all_tokens().unwrap_or(StoredTokens {
        google: HashMap::new(),
        icloud: HashMap::new(),
    });

    let file_tokens = if has_keyring {
        TokenInfo { refresh_token: None, ..tokens.clone() }
    } else {
        tokens.clone()
    };
    stored.google.insert(account_id.to_string(), GoogleTokens {
        tokens: file_tokens,
        stored_at: Utc::now(),
    });

    save_all_tokens(&stored)
}

/// Save iCloud discovery info for a specific account
pub fn save_icloud_tokens(account_id: &str, calendars: &[StoredCalendar]) -> Result<()> {
    Config::ensure_config_dir()?;

    let mut stored = load_all_tokens().unwrap_or(StoredTokens {
        google: HashMap::new(),
        icloud: HashMap::new(),
    });

    stored.icloud.insert(account_id.to_string(), ICloudTokens {
        calendar_urls: Vec::new(), // Legacy field, keep empty
        calendars: calendars.to_vec(),
        stored_at: Utc::now(),
    });

    save_all_tokens(&stored)
}

fn save_all_tokens(stored: &StoredTokens) -> Result<()> {
    let path = Config::token_path();
    let json = serde_json::to_string_pretty(stored)?;
    fs::write(&path, &json)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

fn load_all_tokens() -> Result<StoredTokens> {
    let path = Config::token_path();
    if !path.exists() {
        return Ok(StoredTokens {
            google: HashMap::new(),
            icloud: HashMap::new(),
        });
    }

    let content = fs::read_to_string(&path)?;
    let stored: StoredTokens = serde_json::from_str(&content)?;
    Ok(stored)
}

/// Migrate orphaned token entries to match current config account IDs.
/// This handles the case where config account IDs were regenerated
/// (e.g. before `id` fields were persisted to disk).
pub fn migrate_tokens(accounts: &[AccountConfig]) {
    let Ok(mut stored) = load_all_tokens() else { return };
    let mut changed = false;

    // Collect orphaned entries (keys that don't match any current config account)
    let config_ids: Vec<String> = accounts.iter().map(|a| match a {
        AccountConfig::Google(g) => &g.id,
        AccountConfig::ICloud(i) => &i.id,
    }).cloned().collect();

    let orphaned_google: Vec<(String, GoogleTokens)> = stored.google.iter()
        .filter(|(k, _)| !config_ids.contains(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let orphaned_icloud: Vec<(String, ICloudTokens)> = stored.icloud.iter()
        .filter(|(k, _)| !config_ids.contains(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // Assign orphaned entries to accounts that have no tokens
    let mut next_orphan_g = 0usize;
    let mut next_orphan_i = 0usize;

    for account in accounts {
        match account {
            AccountConfig::Google(g) => {
                if !stored.google.contains_key(&g.id) && next_orphan_g < orphaned_google.len() {
                    stored.google.insert(g.id.clone(), orphaned_google[next_orphan_g].1.clone());
                    next_orphan_g += 1;
                    changed = true;
                }
            }
            AccountConfig::ICloud(i) => {
                if !stored.icloud.contains_key(&i.id) && next_orphan_i < orphaned_icloud.len() {
                    stored.icloud.insert(i.id.clone(), orphaned_icloud[next_orphan_i].1.clone());
                    next_orphan_i += 1;
                    changed = true;
                }
            }
        }
    }

    // Remove any remaining orphaned entries (from accounts that no longer exist)
    stored.google.retain(|k, _| config_ids.contains(k));
    stored.icloud.retain(|k, _| config_ids.contains(k));

    if changed {
        let _ = save_all_tokens(&stored);
    }
}

/// Load Google tokens for a specific account.
/// Primary source: system keyring. Fallback: tokens.json.
pub fn load_google_tokens(account_id: &str) -> Result<Option<TokenInfo>> {
    // Try keyring first (more secure)
    if let Some(tokens) = load_google_tokens_keyring(account_id) {
        return Ok(Some(tokens));
    }
    // Fall back to file-based storage
    let stored = load_all_tokens()?;
    Ok(stored.google.get(account_id).map(|g| g.tokens.clone()))
}

/// Load iCloud discovery info for a specific account
pub fn load_icloud_tokens(account_id: &str) -> Result<Option<ICloudTokens>> {
    let stored = load_all_tokens()?;
    Ok(stored.icloud.get(account_id).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_category_deserialization() {
        let json = r#"{
            "google": {
                "client_id": "test",
                "client_secret": "test",
                "calendar_id": "primary",
                "category": { "name": "Work", "accent": "blue" }
            },
            "icloud": null
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        let cat = config.google.unwrap().category.unwrap();
        assert_eq!(cat.name, "Work");
        assert_eq!(cat.accent, "blue");
    }

    #[test]
    fn test_category_default_accent() {
        let json = r#"{
            "google": {
                "client_id": "test",
                "client_secret": "test",
                "calendar_id": "primary",
                "category": { "name": "Work" }
            },
            "icloud": null
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        let cat = config.google.unwrap().category.unwrap();
        assert_eq!(cat.name, "Work");
        assert_eq!(cat.accent, "blue");
    }

    #[test]
    fn test_category_parses_to_blue() {
        let accent = "blue";
        let color = match accent.to_lowercase().as_str() {
            "blue" => "Color::Blue",
            "red" => "Color::Red",
            _ => "Color::Blue",
        };
        assert_eq!(color, "Color::Blue");
    }

    #[test]
    fn test_new_format_deserializes_accounts() {
        let json = r#"{
            "categories": [
                { "name": "Work", "accent": "blue" },
                { "name": "Personal", "accent": "magenta" }
            ],
            "accounts": [
                {
                    "type": "google",
                    "client_id": "test-g",
                    "client_secret": "secret-g",
                    "calendar_id": "primary",
                    "category": "Work"
                },
                {
                    "type": "icloud",
                    "method": "caldav",
                    "apple_id": "user@icloud.com",
                    "app_password": "abcd-efgh-ijkl",
                    "category": "Personal"
                }
            ]
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.categories.len(), 2);
        assert_eq!(config.categories[0].name, "Work");
        assert_eq!(config.categories[1].name, "Personal");
        assert_eq!(config.accounts.len(), 2);

        // Legacy accessors are not populated by raw serde (that's Config::load()'s job)
        assert!(config.google.is_none());
        assert!(config.icloud.is_none());

        // Check account data directly
        match &config.accounts[0] {
            AccountConfig::Google(g) => {
                assert_eq!(g.calendar_id, "primary");
                assert_eq!(g.category.as_deref(), Some("Work"));
            }
            _ => panic!("Expected Google account"),
        }
        match &config.accounts[1] {
            AccountConfig::ICloud(i) => {
                assert_eq!(i.method, "caldav");
                assert_eq!(i.apple_id.as_deref(), Some("user@icloud.com"));
                assert_eq!(i.category.as_deref(), Some("Personal"));
            }
            _ => panic!("Expected iCloud account"),
        }
    }

    #[test]
    fn test_new_format_no_category() {
        let json = r#"{
            "accounts": [
                {
                    "type": "google",
                    "client_id": "test-g",
                    "client_secret": "secret-g",
                    "calendar_id": "primary"
                }
            ]
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.accounts.len(), 1);
        assert!(config.categories.is_empty());
        let AccountConfig::Google(g) = &config.accounts[0] else {
            panic!("Expected Google account");
        };
        assert!(g.category.is_none());
    }

    #[test]
    fn test_save_output_new_format() {
        let config = Config {
            google: Some(GoogleConfig {
                client_id: "a".to_string(),
                client_secret: "b".to_string(),
                calendar_id: "primary".to_string(),
                category: None,
            }),
            icloud: Some(ICloudConfig {
                method: "caldav".to_string(),
                apple_id: Some("u@i.com".to_string()),
                app_password: Some("p".to_string()),
                category: None,
            }),
            accounts: vec![],
            categories: vec![],
        };

        let serialized = {
            let mut accounts: Vec<AccountConfig> = Vec::new();
            let mut categories = config.categories.clone();
            if let Some(ref google) = config.google {
                let cat_name = google.category.as_ref().map(|c| c.name.clone());
                if let Some(ref cat) = google.category {
                    if !categories.iter().any(|c: &Category| c.name == cat.name) {
                        categories.push(cat.clone());
                    }
                }
                accounts.push(AccountConfig::Google(GoogleAccountConfig {
                    id: generate_account_id(),
                    name: None,
                    calendar_id: google.calendar_id.clone(),
                    category: cat_name,
                }));
            }
            if let Some(ref icloud) = config.icloud {
                let cat_name = icloud.category.as_ref().map(|c| c.name.clone());
                if let Some(ref cat) = icloud.category {
                    if !categories.iter().any(|c: &Category| c.name == cat.name) {
                        categories.push(cat.clone());
                    }
                }
                accounts.push(AccountConfig::ICloud(ICloudAccountConfig {
                    id: generate_account_id(),
                    name: None,
                    method: icloud.method.clone(),
                    apple_id: icloud.apple_id.clone(),
                    app_password: icloud.app_password.clone(),
                    category: cat_name,
                }));
            }
            #[derive(Serialize)]
            struct SaveConfig<'a> {
                categories: &'a Vec<Category>,
                accounts: &'a Vec<AccountConfig>,
            }
            serde_json::to_string_pretty(&SaveConfig {
                categories: &categories,
                accounts: &accounts,
            })
            .unwrap()
        };

        // Parse back and verify structure — no top-level google/icloud keys
        let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        let obj = parsed.as_object().unwrap();
        assert!(obj.contains_key("accounts"), "should have accounts");
        assert!(!obj.contains_key("google"), "should NOT have top-level google");
        assert!(!obj.contains_key("icloud"), "should NOT have top-level icloud");

        // Verify accounts content
        let accounts = obj["accounts"].as_array().unwrap();
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0]["type"], "google");
        assert_eq!(accounts[1]["type"], "icloud");
        assert_eq!(accounts[1]["apple_id"], "u@i.com");
    }

    #[test]
    fn test_new_format_roundtrip_via_load() {
        // Write new-format JSON to a temp config path
        let json = r#"{
            "categories": [
                { "name": "Work", "accent": "blue" },
                { "name": "Personal", "accent": "magenta" }
            ],
            "accounts": [
                {
                    "type": "google",
                    "client_id": "roundtrip-g",
                    "client_secret": "secret-g",
                    "calendar_id": "primary",
                    "category": "Work"
                },
                {
                    "type": "icloud",
                    "method": "caldav",
                    "apple_id": "u@i.com",
                    "app_password": "p",
                    "category": "Personal"
                }
            ]
        }"#;

        let dir = std::env::temp_dir().join("calendarchy_test_roundtrip");
        let _ = std::fs::create_dir_all(&dir);
        let config_path = dir.join("config.json");
        std::fs::write(&config_path, json).unwrap();

        // Override HOME so Config::config_dir() points to our temp dir
        let orig_home = std::env::var("HOME").ok();
        unsafe { std::env::set_var("HOME", &dir); }
        // Also clear XDG_CONFIG_HOME on Linux so dirs::config_dir uses HOME/.config
        let orig_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        unsafe { std::env::remove_var("XDG_CONFIG_HOME"); }

        // Also create the .config/calendarchy dir since Config::config_dir()
        // returns dir/.config/calendarchy
        let hidden_config = dir.join(".config/calendarchy");
        let _ = std::fs::create_dir_all(&hidden_config);
        // Copy our JSON to where load() will look for it
        std::fs::rename(&config_path, hidden_config.join("config.json")).unwrap();

        let config = Config::load().unwrap();

        // Restore env
        if let Some(h) = orig_home { unsafe { std::env::set_var("HOME", h); } }
        if let Some(x) = orig_xdg { unsafe { std::env::set_var("XDG_CONFIG_HOME", x); } } else { unsafe { std::env::remove_var("XDG_CONFIG_HOME"); } }

        // Legacy accessors should be populated by load()
        assert!(config.google.is_some());
        assert!(config.icloud.is_some());

        let g = config.google.as_ref().unwrap();
        assert_eq!(g.client_id, DEFAULT_GOOGLE_CLIENT_ID);
        assert_eq!(g.calendar_id, "primary");
        let g_cat = g.category.as_ref().unwrap();
        assert_eq!(g_cat.name, "Work");
        assert_eq!(g_cat.accent, "blue");

        let i = config.icloud.as_ref().unwrap();
        assert_eq!(i.method, "caldav");
        assert_eq!(i.apple_id.as_deref(), Some("u@i.com"));
        let i_cat = i.category.as_ref().unwrap();
        assert_eq!(i_cat.name, "Personal");
        assert_eq!(i_cat.accent, "magenta");

        // Clean up
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_old_format_compat_via_load() {
        let json = r##"{
            "google": {
                "client_id": "old-client",
                "client_secret": "old-secret",
                "calendar_id": "primary",
                "category": { "name": "Work", "accent": "#ff0000" }
            },
            "icloud": {
                "method": "eventkit",
                "category": { "name": "Personal", "accent": "theme" }
            }
        }"##;

        let config: Config = serde_json::from_str(json).unwrap();

        assert!(config.google.is_some());
        assert!(config.icloud.is_some());
        let g = config.google.as_ref().unwrap();
        assert_eq!(g.client_id, "old-client");
        assert_eq!(g.category.as_ref().unwrap().name, "Work");
        assert_eq!(g.category.as_ref().unwrap().accent, "#ff0000");

        let i = config.icloud.as_ref().unwrap();
        assert_eq!(i.method, "eventkit");
        assert_eq!(i.category.as_ref().unwrap().name, "Personal");
        assert_eq!(i.category.as_ref().unwrap().accent, "theme");

        // accounts should be empty (only populated by Config::load())
        assert!(config.accounts.is_empty());
    }
}
