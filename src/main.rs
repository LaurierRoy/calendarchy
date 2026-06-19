mod app;
mod auth;
mod cache;
mod config;
mod conversion;
mod error;
mod eventkit;
mod google;
mod icloud;
mod logging;
mod setup;
mod ui;
mod utils;

use app::{App, ICloudMethod, NavigationMode, PendingAction, SetupState, SetupStep};
use auth::{AccountAuthState, CalendarEntry, GoogleAuthState, ICloudAuthState};
use cache::{DisplayEvent, EventId};
use chrono::{NaiveDate, Timelike};
use config::{AccountConfig, Config, GoogleConfig, ICloudConfig};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    style::Color,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use google::{CalendarClient, GoogleAuth, TokenInfo};
use icloud::{CalDavClient, ICalEvent, ICloudAuth};
use std::io::stdout;
use std::time::Duration as StdDuration;
use utils::open_url;
use tokio::sync::mpsc;

fn omarchy_theme_accent() -> Option<Color> {
    let config_dir = dirs::config_dir()?;
    let data_dir = dirs::data_dir()?;

    let theme_name = std::fs::read_to_string(config_dir.join("omarchy").join("current").join("theme.name"))
        .ok()?
        .trim()
        .to_string();

    let paths = [
        config_dir.join("omarchy").join("themes").join(&theme_name).join("colors.toml"),
        data_dir.join("omarchy").join("themes").join(&theme_name).join("colors.toml"),
    ];

    let content = paths.iter().find_map(|p| std::fs::read_to_string(p).ok())?;

    for line in content.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("accent = ") {
            let hex = val.trim().trim_matches('"').trim_matches('\'');
            if hex.len() == 7 && hex.starts_with('#') {
                if let Ok(r) = u8::from_str_radix(&hex[1..3], 16) {
                    if let Ok(g) = u8::from_str_radix(&hex[3..5], 16) {
                        if let Ok(b) = u8::from_str_radix(&hex[5..7], 16) {
                            return Some(Color::Rgb { r, g, b });
                        }
                    }
                }
            }
        }
    }
    None
}

fn parse_accent_color(accent: &str) -> Color {
    if accent.starts_with('#') && accent.len() == 7 {
        if let Ok(r) = u8::from_str_radix(&accent[1..3], 16) {
            if let Ok(g) = u8::from_str_radix(&accent[3..5], 16) {
                if let Ok(b) = u8::from_str_radix(&accent[5..7], 16) {
                    return Color::Rgb { r, g, b };
                }
            }
        }
    }
    match accent.to_lowercase().as_str() {
        "theme" => omarchy_theme_accent().unwrap_or(Color::Blue),
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        _ => Color::Blue,
    }
}

fn find_category<'a>(config: &'a Config, account: &AccountConfig) -> Option<&'a config::Category> {
    let cat_name = match account {
        AccountConfig::Google(g) => g.category.as_deref(),
        AccountConfig::ICloud(i) => i.category.as_deref(),
    };
    cat_name.and_then(|name| config.categories.iter().find(|c| c.name == name))
}

fn account_accent_color(config: &Config, account: &AccountConfig) -> Color {
    let cat = find_category(config, account);
    match account {
        AccountConfig::Google(_) => cat.map_or(Color::Blue, |c| parse_accent_color(&c.accent)),
        AccountConfig::ICloud(_) => cat.map_or(Color::Magenta, |c| parse_accent_color(&c.accent)),
    }
}

fn account_label(account: &AccountConfig) -> String {
    match account {
        AccountConfig::Google(g) => g.name.clone().unwrap_or_else(|| "Google".to_string()),
        AccountConfig::ICloud(i) => i.name.clone().unwrap_or_else(|| "iCloud".to_string()),
    }
}

fn account_detail_label(config: &Config, account_idx: usize) -> Option<String> {
    let account = config.accounts.get(account_idx)?;
    let cat = find_category(config, account).map(|c| c.name.as_str());
    let name = match account {
        AccountConfig::Google(g) => g.name.as_deref(),
        AccountConfig::ICloud(i) => i.name.as_deref(),
    };
    match (cat, name) {
        (Some(c), Some(n)) => Some(format!("{} - {}", c, n)),
        (Some(c), None) => Some(c.to_string()),
        (None, Some(n)) => Some(n.to_string()),
        (None, None) => match account {
            AccountConfig::Google(_) => Some("Google".to_string()),
            AccountConfig::ICloud(_) => Some("iCloud".to_string()),
        },
    }
}

fn generate_account_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("a{:x}", nanos)
}

enum AsyncMessage {
    GoogleToken(usize, TokenInfo),
    GoogleAuthError(usize, String),
    GoogleEvents(usize, Vec<google::CalendarEvent>, NaiveDate, String, Option<String>),
    GoogleFetchError(usize, String),
    GoogleTokenRefreshed(usize, TokenInfo),
    GoogleRefreshFailed(usize, String),
    ICloudDiscovered { account_idx: usize, calendars: Vec<CalendarEntry> },
    ICloudDiscoveryError(usize, String),
    ICloudEvents(usize, Vec<(ICalEvent, Option<String>)>, NaiveDate),
    ICloudFetchError(usize, String),
    EventKitEvents(usize, Vec<DisplayEvent>, NaiveDate),
    EventKitError(usize, String),
    EventActionSuccess(String),
    EventActionError(String),
}

fn start_google_auth(app: &mut App, gc: GoogleConfig, account_idx: usize, tx: &mpsc::Sender<AsyncMessage>) {
    app.account_auths[account_idx] = AccountAuthState::Google(GoogleAuthState::Authenticating);
    app.set_status("Opening browser for Google sign-in...");
    let auth = GoogleAuth::new(gc);
    let url = auth.auth_url();
    open_url(&url);
    let tx = tx.clone();
    tokio::spawn(async move {
        match auth.authenticate_with_browser().await {
            Ok(tokens) => {
                let _ = tx.send(AsyncMessage::GoogleToken(account_idx, tokens)).await;
            }
            Err(e) => {
                let _ = tx.send(AsyncMessage::GoogleAuthError(account_idx, e.to_string())).await;
            }
        }
    });
}

fn start_auth_for_account(app: &mut App, account_idx: usize, tx: &mpsc::Sender<AsyncMessage>) {
    if account_idx >= app.config.accounts.len() {
        return;
    }
    if app.account_auths[account_idx].is_authenticated() {
        return;
    }
    match app.config.accounts[account_idx].clone() {
        AccountConfig::Google(g) => {
            let gc = GoogleConfig {
                client_id: g.client_id,
                client_secret: g.client_secret,
                calendar_id: g.calendar_id,
                category: None,
            };
            start_google_auth(app, gc, account_idx, tx);
        }
        AccountConfig::ICloud(icloud) => {
            if icloud.method == "eventkit" {
                app.account_auths[account_idx] = AccountAuthState::ICloud(ICloudAuthState::Authenticated { calendars: vec![] });
                app.needs_fetch[account_idx] = true;
            } else {
                app.account_auths[account_idx] = AccountAuthState::ICloud(ICloudAuthState::Discovering);
                let icloud_config = ICloudConfig {
                    method: icloud.method,
                    apple_id: icloud.apple_id,
                    app_password: icloud.app_password,
                    category: None,
                };
                let auth = ICloudAuth::new(icloud_config);
                let client = CalDavClient::new(auth);
                let tx = tx.clone();
                tokio::spawn(async move {
                    match client.discover_calendars().await {
                        Ok(discovered) => {
                            let calendars: Vec<CalendarEntry> = discovered
                                .into_iter()
                                .map(|c| CalendarEntry { url: c.url, name: c.name })
                                .collect();
                            if calendars.is_empty() {
                                let _ = tx.send(AsyncMessage::ICloudDiscoveryError(
                                    account_idx, "No calendars found".to_string()
                                )).await;
                            } else {
                                let _ = tx.send(AsyncMessage::ICloudDiscovered { account_idx, calendars }).await;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(AsyncMessage::ICloudDiscoveryError(account_idx, e.to_string())).await;
                        }
                    }
                });
            }
        }
    }
}

fn open_setup_wizard(app: &mut App) {
    let mut setup = SetupState::new();
    setup.eventkit_available = eventkit::is_available();

    if !app.config.accounts.is_empty() {
        setup.step = SetupStep::GoogleAsk;
    } else if setup::should_show_shortcut_step() {
        setup.step = SetupStep::ShortcutAsk;
        #[cfg(target_os = "macos")]
        {
            setup.available_terminals = setup::detect_terminal_names();
        }
    } else {
        setup.step = SetupStep::Welcome;
    }

    app.setup = Some(setup);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("calendarchy {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    if std::env::args().any(|a| a == "--remove-setup") {
        return setup::remove_setup();
    }

    let mut app = App::new();

    app.config = Config::load().unwrap_or_default();
    app.resize_accounts(app.config.accounts.len());
    app.events.load_from_disk();

    // Save config to persist any generated account IDs so they're stable across restarts
    let _ = app.config.save();

    // Migrate any orphaned token entries to the current config account IDs
    config::migrate_tokens(&app.config.accounts);

    let mut google_refresh_tokens: Vec<(usize, String, String, String, String)> = Vec::new();

    for (i, account) in app.config.accounts.iter().enumerate() {
        match account {
            AccountConfig::Google(g) => {
                app.account_auths[i] = AccountAuthState::Google(GoogleAuthState::NotAuthenticated);
                if let Ok(Some(tokens)) = config::load_google_tokens(&g.id) {
                    if !tokens.is_expired() {
                        app.account_auths[i] = AccountAuthState::Google(GoogleAuthState::Authenticated(tokens));
                        app.needs_fetch[i] = true;
                    } else if let Some(ref refresh_token) = tokens.refresh_token {
                        google_refresh_tokens.push((
                            i,
                            g.id.clone(),
                            g.client_id.clone(),
                            g.client_secret.clone(),
                            refresh_token.clone(),
                        ));
                        app.loading[i] = true;
                    }
                }
            }
            AccountConfig::ICloud(icloud) => {
                if icloud.method == "eventkit" {
                    app.account_auths[i] = AccountAuthState::ICloud(ICloudAuthState::Authenticated { calendars: vec![] });
                    app.needs_fetch[i] = true;
                } else {
                    app.account_auths[i] = AccountAuthState::ICloud(ICloudAuthState::NotAuthenticated);
                    if let Ok(Some(icloud_tokens)) = config::load_icloud_tokens(&icloud.id) {
                        let calendars: Vec<CalendarEntry> = if !icloud_tokens.calendars.is_empty() {
                            icloud_tokens.calendars.into_iter()
                                .map(|c| CalendarEntry { url: c.url, name: c.name })
                                .collect()
                        } else {
                            icloud_tokens.calendar_urls.into_iter()
                                .map(|url| CalendarEntry { url, name: None })
                                .collect()
                        };
                        if !calendars.is_empty() {
                            app.account_auths[i] = AccountAuthState::ICloud(ICloudAuthState::Authenticated { calendars });
                            app.needs_fetch[i] = true;
                        }
                    }
                }
            }
        }
    }

    if app.config.accounts.is_empty() {
        open_setup_wizard(&mut app);
    }

    let (tx, mut rx) = mpsc::channel::<AsyncMessage>(32);

    for (i, account_id, client_id, client_secret, refresh_token) in google_refresh_tokens {
        let auth = GoogleAuth::new(GoogleConfig {
            client_id,
            client_secret,
            calendar_id: "primary".to_string(),
            category: None,
        });
        let tx = tx.clone();
        tokio::spawn(async move {
            match auth.refresh_token(&refresh_token).await {
                Ok(new_tokens) => {
                    let _ = config::save_google_tokens(&account_id, &new_tokens);
                    let _ = tx.send(AsyncMessage::GoogleTokenRefreshed(i, new_tokens)).await;
                }
                Err(e) => {
                    let _ = tx.send(AsyncMessage::GoogleRefreshFailed(i, e.to_string())).await;
                }
            }
        });
    }

    for (i, account) in app.config.accounts.iter().enumerate() {
        if let AccountConfig::ICloud(icloud) = account {
            if icloud.method != "eventkit"
                && matches!(app.account_auths[i], AccountAuthState::ICloud(ICloudAuthState::NotAuthenticated))
            {
                app.account_auths[i] = AccountAuthState::ICloud(ICloudAuthState::Discovering);
                let icloud_config = ICloudConfig {
                    method: icloud.method.clone(),
                    apple_id: icloud.apple_id.clone(),
                    app_password: icloud.app_password.clone(),
                    category: None,
                };
                let auth = ICloudAuth::new(icloud_config);
                let client = CalDavClient::new(auth);
                let tx = tx.clone();
                tokio::spawn(async move {
                    match client.discover_calendars().await {
                        Ok(discovered) => {
                            let calendars: Vec<CalendarEntry> = discovered
                                .into_iter()
                                .map(|c| CalendarEntry { url: c.url, name: c.name })
                                .collect();
                            if calendars.is_empty() {
                                let _ = tx.send(AsyncMessage::ICloudDiscoveryError(
                                    i, "No calendars found".to_string()
                                )).await;
                            } else {
                                let _ = tx.send(AsyncMessage::ICloudDiscovered { account_idx: i, calendars }).await;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(AsyncMessage::ICloudDiscoveryError(i, e.to_string())).await;
                        }
                    }
                });
            }
        }
    }

    // Show helpful status if any accounts need auth
    // Show helpful status if any accounts need auth
    let account_labels: Vec<String> = app.config.accounts.iter().map(|a| account_label(a)).collect();
    let unauth_indices: Vec<usize> = app.account_auths.iter().enumerate()
        .filter_map(|(i, a)| if !a.is_authenticated() { Some(i) } else { None })
        .collect();
    if !unauth_indices.is_empty() {
        let names: Vec<&str> = unauth_indices.iter().map(|&i| account_labels[i].as_str()).collect();
        app.set_status(format!("Press 1-{} to sign in: {}", names.len(), names.join(", ")));
    }

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, cursor::Hide)?;

    loop {
        if app.clear_expired_status() {
            app.dirty = true;
        }

        let now_minute = chrono::Local::now().minute();
        if now_minute != app.last_render_minute {
            app.last_render_minute = now_minute;
            app.dirty = true;
        }

        if app.dirty {
            app.dirty = false;

            let account_labels: Vec<String> = app.config.accounts.iter()
                .map(|a| {
                    let cat_name = match a {
                        AccountConfig::Google(g) => g.category.as_deref(),
                        AccountConfig::ICloud(i) => i.category.as_deref(),
                    };
                    cat_name
                        .and_then(|name| app.config.categories.iter().find(|c| c.name == name))
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| match a {
                            AccountConfig::Google(_) => "Google".to_string(),
                            AccountConfig::ICloud(_) => "iCloud".to_string(),
                        })
                })
                .collect();
            let account_accents: Vec<Color> = app.config.accounts.iter()
                .map(|a| account_accent_color(&app.config, a))
                .collect();

            let render_state = ui::RenderState {
                current_date: app.current_date,
                selected_date: app.selected_date,
                show_logs: app.show_logs,
                events: &app.events,
                account_auths: &app.account_auths,
                status_message: app.status_message.as_deref(),
                loading: &app.loading,
                navigation_mode: app.navigation_mode,
                selected_source: app.selected_source,
                selected_event_index: app.selected_event_index,
                pending_action: app.pending_action.as_ref(),
                search: app.search.as_ref(),
                setup: app.setup.as_ref(),
                account_labels: &account_labels,
                account_accents: &account_accents,
            };
            ui::render(&render_state);
        }

        for i in 0..app.account_auths.len() {
            if !app.needs_fetch[i] { continue; }
            app.needs_fetch[i] = false;

            let (start, end) = app.month_range();
            if app.events.sources[i].has_month(start) { continue; }
            if !app.account_auths[i].is_authenticated() { continue; }

            app.loading[i] = true;
            app.dirty = true;

            match &app.config.accounts[i] {
                AccountConfig::Google(g) => {
                    if let AccountAuthState::Google(GoogleAuthState::Authenticated(tokens)) = &app.account_auths[i] {
                        let tokens = tokens.clone();
                        let calendar_id = g.calendar_id.clone();
                        let tx = tx.clone();
                        let account_idx = i;
                        tokio::spawn(async move {
                            let client = CalendarClient::new();
                            let calendar_name = client.get_calendar_name(&tokens, &calendar_id).await.ok().flatten();
                            match client.list_events(&tokens, &calendar_id, start, end).await {
                                Ok(events) => {
                                    let _ = tx.send(AsyncMessage::GoogleEvents(account_idx, events, start, calendar_id, calendar_name)).await;
                                }
                                Err(e) => {
                                    let _ = tx.send(AsyncMessage::GoogleFetchError(account_idx, e.to_string())).await;
                                }
                            }
                        });
                    }
                }
                AccountConfig::ICloud(icloud) => {
                    if let AccountAuthState::ICloud(ICloudAuthState::Authenticated { calendars }) = &app.account_auths[i] {
                        if icloud.method == "eventkit" {
                            let tx = tx.clone();
                            let account_idx = i;
                            tokio::spawn(async move {
                                let result = tokio::task::spawn_blocking(move || {
                                    eventkit::fetch_events(start, end)
                                }).await;
                                match result {
                                    Ok(Ok(events)) => {
                                        let _ = tx.send(AsyncMessage::EventKitEvents(account_idx, events, start)).await;
                                    }
                                    Ok(Err(e)) => {
                                        let _ = tx.send(AsyncMessage::EventKitError(account_idx, e)).await;
                                    }
                                    Err(e) => {
                                        let _ = tx.send(AsyncMessage::EventKitError(account_idx, e.to_string())).await;
                                    }
                                }
                            });
                        } else {
                            let icloud_config = ICloudConfig {
                                method: icloud.method.clone(),
                                apple_id: icloud.apple_id.clone(),
                                app_password: icloud.app_password.clone(),
                                category: None,
                            };
                            let auth = ICloudAuth::new(icloud_config);
                            let client = CalDavClient::new(auth);
                            let calendars = calendars.clone();
                            let tx = tx.clone();
                            let account_idx = i;
                            tokio::spawn(async move {
                                let mut all_events: Vec<(ICalEvent, Option<String>)> = Vec::new();
                                for cal in &calendars {
                                    match client.fetch_events(&cal.url, start, end).await {
                                        Ok(events) => {
                                            for e in events {
                                                all_events.push((e, cal.name.clone()));
                                            }
                                        }
                                        Err(e) => {
                                            let _ = tx.send(AsyncMessage::ICloudFetchError(account_idx, e.to_string())).await;
                                            return;
                                        }
                                    }
                                }
                                let _ = tx.send(AsyncMessage::ICloudEvents(account_idx, all_events, start)).await;
                            });
                        }
                    }
                }
            }
        }

        while let Ok(msg) = rx.try_recv() {
            app.dirty = true;
            match msg {
                AsyncMessage::GoogleToken(account_idx, tokens) => {
                    let account_id = match app.config.accounts.get(account_idx) {
                        Some(AccountConfig::Google(g)) => g.id.clone(),
                        _ => String::new(),
                    };
                    if !account_id.is_empty() {
                        let _ = config::save_google_tokens(&account_id, &tokens);
                    }
                    app.account_auths[account_idx] = AccountAuthState::Google(GoogleAuthState::Authenticated(tokens));
                    app.needs_fetch[account_idx] = true;
                    app.set_status("Connected to Google Calendar!");
                    if let Some(ref mut setup) = app.setup {
                        if setup.step == SetupStep::GoogleAuthWaiting {
                            setup.step = SetupStep::ICloudAsk;
                        }
                    }
                }
                AsyncMessage::GoogleAuthError(account_idx, msg) => {
                    app.account_auths[account_idx] = AccountAuthState::Google(GoogleAuthState::Error(msg.clone()));
                    app.set_status(format!("Google: {}", msg));
                    if let Some(ref mut setup) = app.setup {
                        if setup.step == SetupStep::GoogleAuthWaiting {
                            setup.error = Some(format!("Google auth failed: {}", msg));
                            setup.step = SetupStep::ICloudAsk;
                        }
                    }
                }
                AsyncMessage::GoogleEvents(account_idx, events, month_date, calendar_id, calendar_name) => {
                    let label = account_detail_label(&app.config, account_idx);
                    let display_events: Vec<DisplayEvent> = events.into_iter()
                        .filter_map(|e| conversion::google_event_to_display(e, calendar_id.clone(), calendar_name.clone(), label.clone()))
                        .collect();
                    app.events.sources[account_idx].store(display_events, month_date);
                    app.events.save_to_disk();
                    app.loading[account_idx] = false;
                }
                AsyncMessage::GoogleFetchError(account_idx, msg) => {
                    app.set_status(format!("Google: {}", msg));
                    app.loading[account_idx] = false;
                }
                AsyncMessage::GoogleTokenRefreshed(account_idx, tokens) => {
                    let account_id = match app.config.accounts.get(account_idx) {
                        Some(AccountConfig::Google(g)) => g.id.clone(),
                        _ => String::new(),
                    };
                    if !account_id.is_empty() {
                        let _ = config::save_google_tokens(&account_id, &tokens);
                    }
                    app.account_auths[account_idx] = AccountAuthState::Google(GoogleAuthState::Authenticated(tokens));
                    app.needs_fetch[account_idx] = true;
                    app.loading[account_idx] = false;
                }
                AsyncMessage::GoogleRefreshFailed(account_idx, msg) => {
                    app.account_auths[account_idx] = AccountAuthState::Google(GoogleAuthState::NotAuthenticated);
                    app.set_status(format!("Token refresh failed: {}", msg));
                    app.loading[account_idx] = false;
                }

                AsyncMessage::ICloudDiscovered { account_idx, calendars } => {
                    let account_id = match app.config.accounts.get(account_idx) {
                        Some(AccountConfig::ICloud(i)) => i.id.clone(),
                        _ => String::new(),
                    };
                    if !account_id.is_empty() {
                        let stored: Vec<config::StoredCalendar> = calendars.iter()
                            .map(|c| config::StoredCalendar { url: c.url.clone(), name: c.name.clone() })
                            .collect();
                        let _ = config::save_icloud_tokens(&account_id, &stored);
                    }
                    let count = calendars.len();
                    app.account_auths[account_idx] = AccountAuthState::ICloud(ICloudAuthState::Authenticated { calendars });
                    app.needs_fetch[account_idx] = true;
                    app.set_status(format!("Connected to {} iCloud calendar(s)!", count));
                }
                AsyncMessage::ICloudDiscoveryError(account_idx, msg) => {
                    app.account_auths[account_idx] = AccountAuthState::ICloud(ICloudAuthState::Error(msg));
                }
                AsyncMessage::ICloudEvents(account_idx, events, month_date) => {
                    let label = account_detail_label(&app.config, account_idx);
                    let display_events: Vec<DisplayEvent> = events.into_iter()
                        .map(|(e, calendar_name)| conversion::icloud_event_to_display(e, calendar_name, label.clone()))
                        .collect();
                    app.events.sources[account_idx].store(display_events, month_date);
                    app.events.save_to_disk();
                    app.loading[account_idx] = false;
                }
                AsyncMessage::ICloudFetchError(account_idx, msg) => {
                    app.set_status(format!("iCloud: {}", msg));
                    app.loading[account_idx] = false;
                }

                AsyncMessage::EventKitEvents(account_idx, events, month_date) => {
                    app.events.sources[account_idx].store(events, month_date);
                    app.events.save_to_disk();
                    app.loading[account_idx] = false;
                }
                AsyncMessage::EventKitError(account_idx, msg) => {
                    app.set_status(format!("EventKit: {}", msg));
                    app.loading[account_idx] = false;
                }

                AsyncMessage::EventActionSuccess(msg) => {
                    app.set_status(msg);
                    app.events.clear();
                    for nf in &mut app.needs_fetch {
                        *nf = true;
                    }
                    app.exit_event_mode();
                }
                AsyncMessage::EventActionError(msg) => {
                    app.set_status(msg);
                }
            }
        }

        if event::poll(StdDuration::from_millis(100))? {
            match event::read()? {
                Event::Resize(_, _) => {
                    app.dirty = true;
                }
                Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                    app.dirty = true;
                    if app.setup.is_some() {
                        match handle_setup_input(&mut app, key_event.code, &tx) {
                            SetupAction::Continue => {}
                            SetupAction::Quit => break,
                            SetupAction::Finished => {
                                app.config = Config::load().unwrap_or_default();
                                app.setup = None;
                                app.resize_accounts(app.config.accounts.len());

                                for (i, account) in app.config.accounts.clone().iter().enumerate() {
                                    match account {
                                        AccountConfig::Google(g) => {
                                            if !matches!(app.account_auths[i], AccountAuthState::Google(GoogleAuthState::Authenticated(_))) {
                                                if let Ok(Some(tokens)) = config::load_google_tokens(&g.id) {
                                                    if !tokens.is_expired() {
                                                        app.account_auths[i] = AccountAuthState::Google(GoogleAuthState::Authenticated(tokens));
                                                        app.needs_fetch[i] = true;
                                                    } else if let Some(ref rt) = tokens.refresh_token {
                                                        let gc = GoogleConfig {
                                                            client_id: g.client_id.clone(),
                                                            client_secret: g.client_secret.clone(),
                                                            calendar_id: g.calendar_id.clone(),
                                                            category: None,
                                                        };
                                                        let auth = GoogleAuth::new(gc);
                                                        let tx = tx.clone();
                                                        let account_idx = i;
                                                        let account_id = g.id.clone();
                                                        let rt = rt.clone();
                                                        app.loading[i] = true;
                                                        tokio::spawn(async move {
                                                            match auth.refresh_token(&rt).await {
                                                                Ok(new_tokens) => {
                                                                    let _ = config::save_google_tokens(&account_id, &new_tokens);
                                                                    let _ = tx.send(AsyncMessage::GoogleTokenRefreshed(account_idx, new_tokens)).await;
                                                                }
                                                                Err(e) => {
                                                                    let _ = tx.send(AsyncMessage::GoogleRefreshFailed(account_idx, e.to_string())).await;
                                                                }
                                                            }
                                                        });
                                                    } else {
                                                        let gc = GoogleConfig {
                                                            client_id: g.client_id.clone(),
                                                            client_secret: g.client_secret.clone(),
                                                            calendar_id: g.calendar_id.clone(),
                                                            category: None,
                                                        };
                                                        start_google_auth(&mut app, gc, i, &tx);
                                                    }
                                                } else {
                                                    let gc = GoogleConfig {
                                                        client_id: g.client_id.clone(),
                                                        client_secret: g.client_secret.clone(),
                                                        calendar_id: g.calendar_id.clone(),
                                                        category: None,
                                                    };
                                                    start_google_auth(&mut app, gc, i, &tx);
                                                }
                                            }
                                        }
                                        AccountConfig::ICloud(icloud) => {
                                            if matches!(app.account_auths[i],
                                                AccountAuthState::ICloud(ICloudAuthState::NotConfigured | ICloudAuthState::NotAuthenticated))
                                            {
                                                if icloud.method == "eventkit" {
                                                    app.account_auths[i] = AccountAuthState::ICloud(ICloudAuthState::Authenticated { calendars: vec![] });
                                                    app.needs_fetch[i] = true;
                                                } else {
                                                    app.account_auths[i] = AccountAuthState::ICloud(ICloudAuthState::Discovering);
                                                    let icloud_config = ICloudConfig {
                                                        method: icloud.method.clone(),
                                                        apple_id: icloud.apple_id.clone(),
                                                        app_password: icloud.app_password.clone(),
                                                        category: None,
                                                    };
                                                    let auth = ICloudAuth::new(icloud_config);
                                                    let client = CalDavClient::new(auth);
                                                    let tx = tx.clone();
                                                    tokio::spawn(async move {
                                                        match client.discover_calendars().await {
                                                            Ok(discovered) => {
                                                                let calendars: Vec<CalendarEntry> = discovered
                                                                    .into_iter()
                                                                    .map(|c| CalendarEntry { url: c.url, name: c.name })
                                                                    .collect();
                                                                if calendars.is_empty() {
                                                                    let _ = tx.send(AsyncMessage::ICloudDiscoveryError(
                                                                        i, "No calendars found".to_string()
                                                                    )).await;
                                                                } else {
                                                                    let _ = tx.send(AsyncMessage::ICloudDiscovered { account_idx: i, calendars }).await;
                                                                }
                                                            }
                                                            Err(e) => {
                                                                let _ = tx.send(AsyncMessage::ICloudDiscoveryError(i, e.to_string())).await;
                                                            }
                                                        }
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    if app.search.is_some() {
                        match key_event.code {
                            KeyCode::Esc => {
                                app.close_search();
                            }
                            KeyCode::Enter => {
                                app.select_search_result();
                            }
                            KeyCode::Backspace => {
                                if let Some(ref mut search) = app.search {
                                    search.query.pop();
                                }
                                app.update_search_results();
                            }
                            KeyCode::Down | KeyCode::Tab => {
                                if let Some(ref mut search) = app.search {
                                    if !search.results.is_empty() {
                                        search.selected_index = (search.selected_index + 1).min(search.results.len() - 1);
                                    }
                                }
                            }
                            KeyCode::Up | KeyCode::BackTab => {
                                if let Some(ref mut search) = app.search {
                                    search.selected_index = search.selected_index.saturating_sub(1);
                                }
                            }
                            KeyCode::Char(c) => {
                                if let Some(ref mut search) = app.search {
                                    search.query.push(c);
                                }
                                app.update_search_results();
                            }
                            _ => {}
                        }
                        continue;
                    }

                    if let Some(action) = app.pending_action.take() {
                        match key_event.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                                match action {
                                    PendingAction::AcceptEvent { account_idx, calendar_id, event_id } => {
                                        if let AccountAuthState::Google(GoogleAuthState::Authenticated(ref tokens)) = app.account_auths[account_idx] {
                                            let tokens = tokens.clone();
                                            let tx = tx.clone();
                                            tokio::spawn(async move {
                                                let client = CalendarClient::new();
                                                match client.respond_to_event(&tokens, &calendar_id, &event_id, "accepted").await {
                                                    Ok(()) => {
                                                        let _ = tx.send(AsyncMessage::EventActionSuccess("Event accepted".to_string())).await;
                                                    }
                                                    Err(e) => {
                                                        let _ = tx.send(AsyncMessage::EventActionError(format!("Failed to accept: {}", e))).await;
                                                    }
                                                }
                                            });
                                            app.set_status("Accepting event...");
                                        }
                                    }
                                    PendingAction::DeclineEvent { account_idx, calendar_id, event_id } => {
                                        if let AccountAuthState::Google(GoogleAuthState::Authenticated(ref tokens)) = app.account_auths[account_idx] {
                                            let tokens = tokens.clone();
                                            let tx = tx.clone();
                                            tokio::spawn(async move {
                                                let client = CalendarClient::new();
                                                match client.respond_to_event(&tokens, &calendar_id, &event_id, "declined").await {
                                                    Ok(()) => {
                                                        let _ = tx.send(AsyncMessage::EventActionSuccess("Event declined".to_string())).await;
                                                    }
                                                    Err(e) => {
                                                        let _ = tx.send(AsyncMessage::EventActionError(format!("Failed to decline: {}", e))).await;
                                                    }
                                                }
                                            });
                                            app.set_status("Declining event...");
                                        }
                                    }
                                    PendingAction::DeleteGoogleEvent { account_idx, calendar_id, event_id } => {
                                        if let AccountAuthState::Google(GoogleAuthState::Authenticated(ref tokens)) = app.account_auths[account_idx] {
                                            let tokens = tokens.clone();
                                            let tx = tx.clone();
                                            tokio::spawn(async move {
                                                let client = CalendarClient::new();
                                                match client.delete_event(&tokens, &calendar_id, &event_id).await {
                                                    Ok(()) => {
                                                        let _ = tx.send(AsyncMessage::EventActionSuccess("Event deleted".to_string())).await;
                                                    }
                                                    Err(e) => {
                                                        let _ = tx.send(AsyncMessage::EventActionError(format!("Failed to delete: {}", e))).await;
                                                    }
                                                }
                                            });
                                            app.set_status("Deleting event...");
                                        }
                                    }
                                    PendingAction::DeleteICloudEvent { account_idx, calendar_url, event_uid, etag } => {
                                        if let AccountConfig::ICloud(icloud_config) = &app.config.accounts[account_idx] {
                                            let icloud_config = ICloudConfig {
                                                method: icloud_config.method.clone(),
                                                apple_id: icloud_config.apple_id.clone(),
                                                app_password: icloud_config.app_password.clone(),
                                                category: None,
                                            };
                                            let auth = ICloudAuth::new(icloud_config);
                                            let client = CalDavClient::new(auth);
                                            let tx = tx.clone();
                                            tokio::spawn(async move {
                                                match client.delete_event(&calendar_url, &event_uid, etag.as_deref()).await {
                                                    Ok(()) => {
                                                        let _ = tx.send(AsyncMessage::EventActionSuccess("Event deleted".to_string())).await;
                                                    }
                                                    Err(e) => {
                                                        let _ = tx.send(AsyncMessage::EventActionError(format!("Failed to delete: {}", e))).await;
                                                    }
                                                }
                                            });
                                            app.set_status("Deleting event...");
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                app.set_status("Cancelled");
                            }
                            _ => {
                                app.pending_action = Some(action);
                            }
                        }
                        continue;
                    }

                    if app.navigation_mode == NavigationMode::Event {
                        match (key_event.code, key_event.modifiers) {
                            (KeyCode::Char('j') | KeyCode::Char('й') | KeyCode::Down, _) => {
                                app.next_event();
                            }
                            (KeyCode::Char('k') | KeyCode::Char('к') | KeyCode::Up, _) => {
                                app.prev_event();
                            }
                            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                                for _ in 0..10 {
                                    app.next_event();
                                }
                            }
                            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                                for _ in 0..10 {
                                    app.prev_event();
                                }
                            }
                            (KeyCode::Char('J'), _) => {
                                if let Some(event) = app.get_selected_event()
                                    && let Some(ref url) = event.meeting_url {
                                        open_url(url);
                                    }
                            }
                            (KeyCode::Char('a') | KeyCode::Char('а'), _) => {
                                if let Some(event) = app.get_selected_event() {
                                    if let EventId::Google { calendar_id, event_id, .. } = event.id.clone() {
                                        let idx = app.selected_source;
                                        if let AccountAuthState::Google(GoogleAuthState::Authenticated(_)) = &app.account_auths[idx] {
                                            app.pending_action = Some(PendingAction::AcceptEvent { account_idx: idx, calendar_id, event_id });
                                        }
                                    } else {
                                        app.set_status("Accept not supported for iCloud");
                                    }
                                }
                            }
                            (KeyCode::Char('d') | KeyCode::Char('д'), m) if !m.contains(KeyModifiers::CONTROL) => {
                                if let Some(event) = app.get_selected_event() {
                                    if let EventId::Google { calendar_id, event_id, .. } = event.id.clone() {
                                        let idx = app.selected_source;
                                        if let AccountAuthState::Google(GoogleAuthState::Authenticated(_)) = &app.account_auths[idx] {
                                            app.pending_action = Some(PendingAction::DeclineEvent { account_idx: idx, calendar_id, event_id });
                                        }
                                    } else {
                                        app.set_status("Decline not supported for iCloud");
                                    }
                                }
                            }
                            (KeyCode::Char('x') | KeyCode::Char('ь'), _) => {
                                if let Some(event) = app.get_selected_event() {
                                    let idx = app.selected_source;
                                    match event.id.clone() {
                                        EventId::Google { calendar_id, event_id, .. } => {
                                            if let AccountAuthState::Google(GoogleAuthState::Authenticated(_)) = &app.account_auths[idx] {
                                                app.pending_action = Some(PendingAction::DeleteGoogleEvent { account_idx: idx, calendar_id, event_id });
                                            }
                                        }
                                        EventId::ICloud { calendar_url, event_uid, etag, .. } => {
                                            let auth_state = &app.account_auths[idx];
                                            if matches!(auth_state, AccountAuthState::ICloud(_)) {
                                                app.pending_action = Some(PendingAction::DeleteICloudEvent { account_idx: idx, calendar_url, event_uid, etag });
                                            }
                                        }
                                    }
                                }
                            }
                            (KeyCode::Char('t') | KeyCode::Char('т'), _) => {
                                app.goto_today();
                            }
                            (KeyCode::Char('r') | KeyCode::Char('р'), _) => {
                                app.events.clear();
                                for nf in &mut app.needs_fetch {
                                    *nf = true;
                                }
                                app.set_status("Refreshing...");
                            }
                            (KeyCode::Char('n') | KeyCode::Char('н'), _) => {
                                app.goto_now();
                            }
                            (KeyCode::Esc, _) => {
                                app.exit_event_mode();
                            }
                            (KeyCode::Char('D'), _) => {
                                app.show_logs = !app.show_logs;
                            }
                            (KeyCode::Char('f') | KeyCode::Char('ф'), _) => {
                                app.open_search();
                            }
                            (KeyCode::Char(c), _) if c.is_ascii_digit() => {
                                let idx = c.to_digit(10).unwrap_or(0).saturating_sub(1) as usize;
                                if idx < app.config.accounts.len() {
                                    app.selected_source = idx;
                                    start_auth_for_account(&mut app, idx, &tx);
                                }
                            }
                            (KeyCode::Char('S'), _) => {
                                open_setup_wizard(&mut app);
                            }
                            (KeyCode::Char('q') | KeyCode::Char('я'), _) => {
                                break;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    match (key_event.code, key_event.modifiers) {
                        (KeyCode::Char('j') | KeyCode::Char('й') | KeyCode::Down, _) => {
                            app.next_day();
                        }
                        (KeyCode::Char('k') | KeyCode::Char('к') | KeyCode::Up, _) => {
                            app.prev_day();
                        }
                        (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                            app.next_month();
                        }
                        (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                            app.prev_month();
                        }
                        (KeyCode::Enter, _) => {
                            app.enter_event_mode();
                        }
                        (KeyCode::Char('t') | KeyCode::Char('т'), _) => {
                            app.goto_today();
                        }
                        (KeyCode::Char('r') | KeyCode::Char('р'), _) => {
                            app.events.clear();
                            for nf in &mut app.needs_fetch {
                                *nf = true;
                            }
                            app.set_status("Refreshing...");
                        }
                        (KeyCode::Char('n') | KeyCode::Char('н'), _) => {
                            app.goto_now();
                        }
                        (KeyCode::Char('D'), _) => {
                            app.show_logs = !app.show_logs;
                        }
                        (KeyCode::Char('f') | KeyCode::Char('ф'), _) => {
                            app.open_search();
                        }
                        (KeyCode::Char(c), _) if c.is_ascii_digit() => {
                            let idx = c.to_digit(10).unwrap_or(0).saturating_sub(1) as usize;
                            start_auth_for_account(&mut app, idx, &tx);
                        }
                        (KeyCode::Char('g') | KeyCode::Char('г'), _) => {
                            let idx = app.selected_source;
                            start_auth_for_account(&mut app, idx, &tx);
                        }
                        (KeyCode::Char('S'), _) => {
                            open_setup_wizard(&mut app);
                        }
                        (KeyCode::Char('i') | KeyCode::Char('и'), _) => {
                            let idx = app.selected_source;
                            if let Some(AccountConfig::ICloud(icloud)) = app.config.accounts.get(idx) {
                                let icloud_config = ICloudConfig {
                                    method: icloud.method.clone(),
                                    apple_id: icloud.apple_id.clone(),
                                    app_password: icloud.app_password.clone(),
                                    category: None,
                                };
                                app.account_auths[idx] = AccountAuthState::ICloud(ICloudAuthState::Discovering);
                                let auth = ICloudAuth::new(icloud_config);
                                let client = CalDavClient::new(auth);
                                let tx = tx.clone();
                                tokio::spawn(async move {
                                    match client.discover_calendars().await {
                                        Ok(discovered) => {
                                            let calendars: Vec<CalendarEntry> = discovered
                                                .into_iter()
                                                .map(|c| CalendarEntry { url: c.url, name: c.name })
                                                .collect();
                                            if calendars.is_empty() {
                                                let _ = tx.send(AsyncMessage::ICloudDiscoveryError(
                                                    idx, "No calendars found".to_string()
                                                )).await;
                                            } else {
                                                let _ = tx.send(AsyncMessage::ICloudDiscovered { account_idx: idx, calendars }).await;
                                            }
                                        }
                                        Err(e) => {
                                            let _ = tx.send(AsyncMessage::ICloudDiscoveryError(idx, e.to_string())).await;
                                        }
                                    }
                                });
                            }
                        }
                        (KeyCode::Char('q') | KeyCode::Char('я') | KeyCode::Esc, _) => {
                            break;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen, cursor::Show)?;

    Ok(())
}

enum SetupAction {
    Continue,
    Quit,
    Finished,
}

fn handle_setup_input(app: &mut App, key: KeyCode, tx: &mpsc::Sender<AsyncMessage>) -> SetupAction {
    if let Some(ref setup) = app.setup {
        if setup.step == SetupStep::GoogleAsk
            && matches!(key, KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter)
        {
            if !app.config.accounts.iter().any(|a| matches!(a, AccountConfig::Google(_))) {
                let id = generate_account_id();
                app.config.accounts.push(AccountConfig::Google(config::GoogleAccountConfig {
                    id: id.clone(),
                    name: None,
                    client_id: std::env::var("CALENDARCHY_GOOGLE_CLIENT_ID")
                        .unwrap_or_else(|_| config::DEFAULT_GOOGLE_CLIENT_ID.to_string()),
                    client_secret: std::env::var("CALENDARCHY_GOOGLE_CLIENT_SECRET")
                        .unwrap_or_else(|_| config::DEFAULT_GOOGLE_CLIENT_SECRET.to_string()),
                    calendar_id: "primary".to_string(),
                    category: Some("Work".to_string()),
                }));
                app.resize_accounts(app.config.accounts.len());
                let _ = app.config.save();
            }

            let account_idx = app.config.accounts.iter().position(|a| matches!(a, AccountConfig::Google(_))).unwrap_or(0);
            if let Some(AccountConfig::Google(g)) = app.config.accounts.get(account_idx) {
                let gc = GoogleConfig {
                    client_id: g.client_id.clone(),
                    client_secret: g.client_secret.clone(),
                    calendar_id: g.calendar_id.clone(),
                    category: None,
                };
                start_google_auth(app, gc, account_idx, tx);
            }
            let setup = app.setup.as_mut().unwrap();
            setup.google_enabled = true;
            setup.step = SetupStep::GoogleAuthWaiting;
            return SetupAction::Continue;
        }
    }

    let setup = app.setup.as_mut().unwrap();
    setup.error = None;

    match setup.step {
        SetupStep::ShortcutAsk => match key {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                #[cfg(target_os = "macos")]
                {
                    if setup.available_terminals.is_empty() {
                        setup.error = Some("No supported terminals found".to_string());
                    } else {
                        setup.step = SetupStep::ShortcutTerminalChoice;
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    match setup::install_shortcut() {
                        Ok(()) => setup.step = SetupStep::Welcome,
                        Err(e) => setup.error = Some(format!("Failed: {}", e)),
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                setup.step = SetupStep::Welcome;
            }
            KeyCode::Char('q') | KeyCode::Esc => return SetupAction::Quit,
            _ => {}
        },
        SetupStep::ShortcutTerminalChoice => match key {
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let idx = (c as u8 - b'1') as usize;
                if idx < setup.available_terminals.len() {
                    #[cfg(target_os = "macos")]
                    match setup::install_shortcut(idx) {
                        Ok(()) => setup.step = SetupStep::Welcome,
                        Err(e) => setup.error = Some(e),
                    }
                }
            }
            KeyCode::Esc => { setup.step = SetupStep::ShortcutAsk; }
            _ => {}
        },
        SetupStep::Welcome => match key {
            KeyCode::Enter => {
                setup.step = SetupStep::GoogleAsk;
            }
            KeyCode::Char('q') | KeyCode::Esc => return SetupAction::Quit,
            _ => {}
        },
        SetupStep::GoogleAsk => match key {
            KeyCode::Char('n') | KeyCode::Char('N') => {
                setup.step = SetupStep::ICloudAsk;
            }
            KeyCode::Esc => { setup.step = SetupStep::Welcome; }
            _ => {}
        },
        SetupStep::GoogleAuthWaiting => match key {
            KeyCode::Esc => {
                setup.step = SetupStep::ICloudAsk;
            }
            _ => {}
        },
        SetupStep::ICloudAsk => match key {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                if setup.eventkit_available {
                    setup.step = SetupStep::ICloudMethod;
                } else {
                    setup.step = SetupStep::ICloudOpenUrl;
                    open_url("https://appleid.apple.com/account/manage");
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                setup.step = SetupStep::Done;
            }
            KeyCode::Esc => {
                setup.step = SetupStep::GoogleAsk;
            }
            _ => {}
        },
        SetupStep::ICloudMethod => match key {
            KeyCode::Char('1') | KeyCode::Enter => {
                setup.icloud_method = Some(ICloudMethod::EventKit);
                setup.step = SetupStep::Done;
            }
            KeyCode::Char('2') => {
                setup.icloud_method = Some(ICloudMethod::CalDav);
                setup.step = SetupStep::ICloudOpenUrl;
                open_url("https://appleid.apple.com/account/manage");
            }
            KeyCode::Esc => { setup.step = SetupStep::ICloudAsk; }
            _ => {}
        },
        SetupStep::ICloudOpenUrl => match key {
            KeyCode::Enter => { setup.step = SetupStep::ICloudAppleId; }
            KeyCode::Esc => {
                if setup.eventkit_available {
                    setup.step = SetupStep::ICloudMethod;
                } else {
                    setup.step = SetupStep::ICloudAsk;
                }
            }
            _ => {}
        },
        SetupStep::ICloudAppleId => match key {
            KeyCode::Enter => {
                let val = setup.input.trim().to_string();
                if val.is_empty() {
                    setup.error = Some("Apple ID cannot be empty".to_string());
                } else {
                    setup.icloud_apple_id = Some(val);
                    setup.input.clear();
                    setup.step = SetupStep::ICloudPassword;
                }
            }
            KeyCode::Esc => {
                setup.input.clear();
                setup.step = SetupStep::ICloudOpenUrl;
            }
            KeyCode::Backspace => { setup.input.pop(); }
            KeyCode::Char(c) => { setup.input.push(c); }
            _ => {}
        },
        SetupStep::ICloudPassword => match key {
            KeyCode::Enter => {
                let val = setup.input.trim().to_string();
                if val.is_empty() {
                    setup.error = Some("App password cannot be empty".to_string());
                } else {
                    setup.icloud_password = Some(val);
                    setup.input.clear();
                    setup.step = SetupStep::Done;
                }
            }
            KeyCode::Esc => {
                setup.input.clear();
                setup.step = SetupStep::ICloudAppleId;
            }
            KeyCode::Backspace => { setup.input.pop(); }
            KeyCode::Char(c) => { setup.input.push(c); }
            _ => {}
        },
        SetupStep::Done => {}
    }

    if setup.step == SetupStep::Done {
        let has_google = setup.google_enabled;
        let has_eventkit = setup.icloud_method == Some(ICloudMethod::EventKit);
        let has_caldav = setup.icloud_apple_id.is_some();
        let already_has_google = app.config.accounts.iter().any(|a| matches!(a, AccountConfig::Google(_)));
        let already_has_icloud = app.config.accounts.iter().any(|a| matches!(a, AccountConfig::ICloud(_)));

        if !has_google && !has_eventkit && !has_caldav && !already_has_google && !already_has_icloud {
            setup.error = Some("Set up at least one calendar".to_string());
            setup.step = SetupStep::GoogleAsk;
            return SetupAction::Continue;
        }

        if has_google && !already_has_google {
            let id = generate_account_id();
            app.config.accounts.push(AccountConfig::Google(config::GoogleAccountConfig {
                id,
                name: None,
                client_id: std::env::var("CALENDARCHY_GOOGLE_CLIENT_ID")
                    .unwrap_or_else(|_| config::DEFAULT_GOOGLE_CLIENT_ID.to_string()),
                client_secret: std::env::var("CALENDARCHY_GOOGLE_CLIENT_SECRET")
                    .unwrap_or_else(|_| config::DEFAULT_GOOGLE_CLIENT_SECRET.to_string()),
                calendar_id: "primary".to_string(),
                category: Some("Work".to_string()),
            }));
        }
        if has_eventkit && !already_has_icloud {
            app.config.accounts.push(AccountConfig::ICloud(config::ICloudAccountConfig {
                id: generate_account_id(),
                name: None,
                method: "eventkit".to_string(),
                apple_id: None,
                app_password: None,
                category: Some("Personal".to_string()),
            }));
        } else if let (Some(apple_id), Some(app_password)) =
            (setup.icloud_apple_id.take(), setup.icloud_password.take())
        {
            if !already_has_icloud {
                app.config.accounts.push(AccountConfig::ICloud(config::ICloudAccountConfig {
                    id: generate_account_id(),
                    name: None,
                    method: "caldav".to_string(),
                    apple_id: Some(apple_id),
                    app_password: Some(app_password),
                    category: Some("Personal".to_string()),
                }));
            }
        }

        if let Err(e) = app.config.save() {
            setup.error = Some(format!("Failed to save config: {}", e));
            setup.step = SetupStep::GoogleAsk;
            return SetupAction::Continue;
        }

        app.resize_accounts(app.config.accounts.len());
        return SetupAction::Finished;
    }

    SetupAction::Continue
}
