use crate::auth::AccountAuthState;
use crate::cache::{DisplayEvent, EventCache, SourceCache};
use crate::config::Config;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveTime, Timelike};

/// Search state for the interactive search modal
pub struct SearchState {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub selected_index: usize,
    pub scroll_offset: usize,
}

/// Whether a search result matched on title or participant
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MatchType {
    Title,
    Participant,
}

/// A single search result with its source index
pub struct SearchResult {
    pub event: DisplayEvent,
    pub source_idx: usize,  // Index into config.accounts / events.sources
    pub match_type: MatchType,
}

/// Interactive setup wizard step
#[derive(Debug, Clone, PartialEq)]
pub enum SetupStep {
    ShortcutAsk,
    ShortcutTerminalChoice, // macOS only — pick terminal emulator
    Welcome,
    GoogleAsk,
    GoogleAuthWaiting,
    ICloudAsk,
    ICloudMethod,     // Choose EventKit vs CalDAV (macOS only)
    ICloudOpenUrl,
    ICloudAppleId,
    ICloudPassword,
    Done,
}

/// Which iCloud method was chosen
#[derive(Debug, Clone, PartialEq)]
pub enum ICloudMethod {
    EventKit,
    CalDav,
}

/// State for the interactive setup wizard
pub struct SetupState {
    pub step: SetupStep,
    pub input: String,
    pub google_enabled: bool,
    pub icloud_method: Option<ICloudMethod>,
    pub icloud_apple_id: Option<String>,
    pub icloud_password: Option<String>,
    pub error: Option<String>,
    pub eventkit_available: bool,
    pub available_terminals: Vec<String>,
}

impl SetupState {
    pub fn new() -> Self {
        Self {
            step: SetupStep::Welcome,
            input: String::new(),
            google_enabled: false,
            icloud_method: None,
            icloud_apple_id: None,
            icloud_password: None,
            error: None,
            eventkit_available: false,
            available_terminals: Vec::new(),
        }
    }
}

/// Navigation mode for two-level navigation in month view
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NavigationMode {
    Day,   // Navigate between days with h/j/k/l
    Event, // Navigate between events within selected day with j/k
}

/// Pending action awaiting confirmation
#[derive(Debug, Clone)]
pub enum PendingAction {
    AcceptEvent { account_idx: usize, calendar_id: String, event_id: String },
    DeclineEvent { account_idx: usize, calendar_id: String, event_id: String },
    DeleteGoogleEvent { account_idx: usize, calendar_id: String, event_id: String },
    DeleteICloudEvent { account_idx: usize, calendar_url: String, event_uid: String, etag: Option<String> },
}

/// Application state
pub struct App {
    pub current_date: NaiveDate,
    pub selected_date: NaiveDate,
    pub show_logs: bool,
    pub events: EventCache,
    pub account_auths: Vec<AccountAuthState>,
    pub needs_fetch: Vec<bool>,
    pub loading: Vec<bool>,
    pub status_message: Option<String>,
    pub status_message_time: Option<std::time::Instant>,
    pub config: Config,
    pub navigation_mode: NavigationMode,
    pub selected_source: usize,
    pub selected_event_index: usize,
    pub pending_action: Option<PendingAction>,
    pub search: Option<SearchState>,
    pub dirty: bool,
    /// Tracks the last minute we rendered, so the countdown timer triggers a re-render each minute
    pub last_render_minute: u32,
    /// Interactive setup wizard state (None = not in setup mode)
    pub setup: Option<SetupState>,
}

impl App {
    pub fn new() -> Self {
        let today = Local::now().date_naive();
        let events = EventCache::new(0);

        let mut app = Self {
            current_date: today,
            selected_date: today,
            show_logs: false,
            events,
            account_auths: Vec::new(),
            needs_fetch: Vec::new(),
            loading: Vec::new(),
            status_message: None,
            status_message_time: None,
            config: Config::default(),
            navigation_mode: NavigationMode::Day,
            selected_source: 0,
            selected_event_index: 0,
            pending_action: None,
            search: None,
            dirty: true,
            last_render_minute: Local::now().minute(),
            setup: None,
        };

        app.enter_event_mode();
        app
    }

    pub fn resize_accounts(&mut self, num: usize) {
        self.account_auths.resize(num, AccountAuthState::NotConfigured);
        self.needs_fetch.resize(num, false);
        self.loading.resize(num, false);
        while self.events.sources.len() < num {
            self.events.sources.push(SourceCache::new());
        }
        self.events.sources.truncate(num);
        self.selected_source = self.selected_source.min(num.saturating_sub(1));
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
        self.status_message_time = Some(std::time::Instant::now());
    }

    pub fn clear_expired_status(&mut self) -> bool {
        if let Some(time) = self.status_message_time
            && time.elapsed() > std::time::Duration::from_secs(3)
        {
            self.status_message = None;
            self.status_message_time = None;
            return true;
        }
        false
    }

    pub fn next_day(&mut self) {
        self.selected_date += Duration::days(1);
        self.sync_month_if_needed();
    }

    pub fn prev_day(&mut self) {
        self.selected_date -= Duration::days(1);
        self.sync_month_if_needed();
    }

    fn sync_month_if_needed(&mut self) {
        if self.selected_date.month() != self.current_date.month()
            || self.selected_date.year() != self.current_date.year()
        {
            self.current_date = self.selected_date.with_day(1).unwrap();
            for nf in &mut self.needs_fetch {
                *nf = true;
            }
        }
    }

    pub fn goto_today(&mut self) {
        let today = Local::now().date_naive();
        let month_changed = today.month() != self.current_date.month()
            || today.year() != self.current_date.year();
        self.current_date = today;
        self.selected_date = today;
        if month_changed {
            for nf in &mut self.needs_fetch {
                *nf = true;
            }
        }
    }

    pub fn goto_now(&mut self) {
        self.goto_today();
        self.enter_event_mode();
    }

    pub fn month_range(&self) -> (NaiveDate, NaiveDate) {
        let first = self.current_date.with_day(1).unwrap();
        let last = if self.current_date.month() == 12 {
            NaiveDate::from_ymd_opt(self.current_date.year() + 1, 1, 1)
                .unwrap()
                - Duration::days(1)
        } else {
            NaiveDate::from_ymd_opt(self.current_date.year(), self.current_date.month() + 1, 1)
                .unwrap()
                - Duration::days(1)
        };
        (first, last)
    }

    pub fn get_current_source_events(&self) -> &[DisplayEvent] {
        if self.selected_source < self.events.sources.len() {
            self.events.sources[self.selected_source].get(self.selected_date)
        } else {
            &[]
        }
    }

    pub fn get_selected_event(&self) -> Option<&DisplayEvent> {
        if self.navigation_mode == NavigationMode::Event {
            self.get_current_source_events().get(self.selected_event_index)
        } else {
            None
        }
    }

    pub fn enter_event_mode(&mut self) {
        let all_empty = self.events.sources.iter().all(|s| s.get(self.selected_date).is_empty());
        if all_empty {
            return;
        }

        self.navigation_mode = NavigationMode::Event;

        let today = Local::now().date_naive();
        if self.selected_date == today {
            let current_time = Local::now().time();

            // Try to find current or next event across sources
            for idx in 0..self.events.sources.len() {
                let events = self.events.sources[idx].get(self.selected_date);
                if let Some((pos, _)) = find_current_or_next_event(events, current_time) {
                    self.selected_source = idx;
                    self.selected_event_index = pos;
                    return;
                }
            }

            // Find the closest next event across all sources
            let mut best: Option<(usize, usize, &str)> = None; // (source_idx, event_idx, time_str)
            for idx in 0..self.events.sources.len() {
                let events = self.events.sources[idx].get(self.selected_date);
                if let Some((pos, _)) = find_current_or_next_event(events, current_time) {
                    let t = &events[pos].time_str;
                    match best {
                        None => best = Some((idx, pos, t)),
                        Some((_, _, bt)) if t.as_str() < bt => best = Some((idx, pos, t)),
                        _ => {}
                    }
                }
            }
            if let Some((idx, pos, _)) = best {
                self.selected_source = idx;
                self.selected_event_index = pos;
                return;
            }
        }

        // Fall back to first non-empty source
        for idx in 0..self.events.sources.len() {
            let events = self.events.sources[idx].get(self.selected_date);
            if !events.is_empty() {
                self.selected_source = idx;
                self.selected_event_index = 0;
                return;
            }
        }
    }

    pub fn exit_event_mode(&mut self) {
        self.navigation_mode = NavigationMode::Day;
        self.selected_event_index = 0;
    }

    pub fn next_event(&mut self) {
        let current_events = self.get_current_source_events();

        if self.selected_event_index < current_events.len().saturating_sub(1) {
            self.selected_event_index += 1;
            return;
        }

        // Try next source
        let n = self.events.sources.len();
        for offset in 1..n {
            let idx = (self.selected_source + offset) % n;
            let events = self.events.sources[idx].get(self.selected_date);
            if !events.is_empty() {
                self.selected_source = idx;
                self.selected_event_index = 0;
                return;
            }
        }

        // No more events today, go to next day with events
        self.navigate_to_next_day_with_events();
    }

    pub fn prev_event(&mut self) {
        if self.selected_event_index > 0 {
            self.selected_event_index -= 1;
            return;
        }

        // Try previous source
        let n = self.events.sources.len();
        for offset in 1..n {
            let idx = (self.selected_source + n - offset) % n;
            let events = self.events.sources[idx].get(self.selected_date);
            if !events.is_empty() {
                self.selected_source = idx;
                self.selected_event_index = events.len().saturating_sub(1);
                return;
            }
        }

        // No more events today, go to previous day with events
        self.navigate_to_prev_day_with_events();
    }

    fn navigate_to_next_day_with_events(&mut self) {
        let mut check_date = self.selected_date + Duration::days(1);
        let limit = self.selected_date + Duration::days(90);

        while check_date <= limit {
            if self.events.has_events(check_date) {
                self.selected_date = check_date;
                if check_date.month() != self.current_date.month() || check_date.year() != self.current_date.year() {
                    self.current_date = check_date;
                }
                for idx in 0..self.events.sources.len() {
                    let events = self.events.sources[idx].get(check_date);
                    if !events.is_empty() {
                        self.selected_source = idx;
                        self.selected_event_index = 0;
                        return;
                    }
                }
                // Fallback
                self.selected_source = 0;
                self.selected_event_index = 0;
            }
            check_date += Duration::days(1);
        }
    }

    fn navigate_to_prev_day_with_events(&mut self) {
        let mut check_date = self.selected_date - Duration::days(1);
        let limit = self.selected_date - Duration::days(90);

        while check_date >= limit {
            if self.events.has_events(check_date) {
                self.selected_date = check_date;
                if check_date.month() != self.current_date.month() || check_date.year() != self.current_date.year() {
                    self.current_date = check_date;
                }
                for idx in (0..self.events.sources.len()).rev() {
                    let events = self.events.sources[idx].get(check_date);
                    if !events.is_empty() {
                        self.selected_source = idx;
                        self.selected_event_index = events.len().saturating_sub(1);
                        return;
                    }
                }
                // Fallback (shouldn't reach since has_events is true)
                self.selected_source = 0;
                self.selected_event_index = 0;
                return;
            }
            check_date -= Duration::days(1);
        }
    }

    pub fn next_month(&mut self) {
        let (year, month) = if self.current_date.month() == 12 {
            (self.current_date.year() + 1, 1)
        } else {
            (self.current_date.year(), self.current_date.month() + 1)
        };
        self.current_date = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
        self.selected_date = self.current_date;
    }

    pub fn prev_month(&mut self) {
        let (year, month) = if self.current_date.month() == 1 {
            (self.current_date.year() - 1, 12)
        } else {
            (self.current_date.year(), self.current_date.month() - 1)
        };
        self.current_date = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
        self.selected_date = self.current_date;
    }

    pub fn open_search(&mut self) {
        self.search = Some(SearchState {
            query: String::new(),
            results: Vec::new(),
            selected_index: 0,
            scroll_offset: 0,
        });
    }

    pub fn close_search(&mut self) {
        self.search = None;
    }

    pub fn update_search_results(&mut self) {
        let query_lower = {
            let search = match self.search.as_ref() {
                Some(s) => s,
                None => return,
            };
            search.query.to_lowercase()
        };
        let mut results: Vec<SearchResult> = Vec::new();
        let today = Local::now().date_naive();

        if !query_lower.is_empty() {
            for (idx, source) in self.events.sources.iter().enumerate() {
                for event in source.all_events() {
                    if event.date >= today
                        && let Some(match_type) = event_match_type(event, &query_lower)
                    {
                        results.push(SearchResult {
                            event: event.clone(),
                            source_idx: idx,
                            match_type,
                        });
                    }
                }
            }
            results.sort_by(|a, b| {
                let a_title = a.event.title.to_lowercase().contains(&query_lower);
                let b_title = b.event.title.to_lowercase().contains(&query_lower);
                b_title.cmp(&a_title)
                    .then_with(|| a.event.date.cmp(&b.event.date))
                    .then_with(|| a.event.time_str.cmp(&b.event.time_str))
            });
        }

        if let Some(ref mut search) = self.search {
            search.results = results;
            if search.selected_index >= search.results.len() {
                search.selected_index = search.results.len().saturating_sub(1);
            }
            // Clamp scroll offset
            if search.selected_index < search.scroll_offset {
                search.scroll_offset = search.selected_index;
            }
        }
    }

    pub fn select_search_result(&mut self) {
        let (date, source_idx, event_title) = match self.search.as_ref() {
            Some(s) => {
                match s.results.get(s.selected_index) {
                    Some(r) => (r.event.date, r.source_idx, r.event.title.clone()),
                    None => return,
                }
            }
            None => return,
        };

        // Navigate to the date
        let month_changed = date.month() != self.current_date.month()
            || date.year() != self.current_date.year();
        self.selected_date = date;
        if month_changed {
            self.current_date = date.with_day(1).unwrap();
            for nf in &mut self.needs_fetch {
                *nf = true;
            }
        }

        // Enter event mode on the correct source/index
        self.navigation_mode = NavigationMode::Event;
        self.selected_source = source_idx;

        let events = if source_idx < self.events.sources.len() {
            self.events.sources[source_idx].get(date)
        } else {
            &[]
        };
        self.selected_event_index = events.iter()
            .position(|e| e.title == event_title)
            .unwrap_or(0);

        self.close_search();
    }
}

/// Check if an event matches the search query (case-insensitive)
#[cfg(test)]
fn event_matches_query(event: &DisplayEvent, query_lower: &str) -> bool {
    event_match_type(event, query_lower).is_some()
}

/// Determine how an event matches the search query, returning the match type.
/// Title matches take priority over participant matches.
pub fn event_match_type(event: &DisplayEvent, query_lower: &str) -> Option<MatchType> {
    if event.title.to_lowercase().contains(query_lower) {
        return Some(MatchType::Title);
    }
    for attendee in &event.attendees {
        if let Some(ref name) = attendee.name
            && name.to_lowercase().contains(query_lower)
        {
            return Some(MatchType::Participant);
        }
        if attendee.email.to_lowercase().contains(query_lower) {
            return Some(MatchType::Participant);
        }
    }
    None
}

/// Find current or next event in a list, returns (index, is_current)
fn find_current_or_next_event(events: &[DisplayEvent], current_time: NaiveTime) -> Option<(usize, bool)> {
    let mut best_current: Option<(usize, NaiveTime)> = None;
    let mut first_next: Option<usize> = None;

    for (i, event) in events.iter().enumerate() {
        if event.time_str == "All day" {
            continue;
        }

        let parts: Vec<&str> = event.time_str.split(':').collect();
        if parts.len() != 2 {
            continue;
        }
        let hour: u32 = match parts[0].parse() {
            Ok(h) => h,
            Err(_) => continue,
        };
        let minute: u32 = match parts[1].parse() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let event_time = match NaiveTime::from_hms_opt(hour, minute, 0) {
            Some(t) => t,
            None => continue,
        };

        if let Some(ref end_str) = event.end_time_str {
            let end_parts: Vec<&str> = end_str.split(':').collect();
            if end_parts.len() == 2
                && let (Ok(eh), Ok(em)) = (end_parts[0].parse::<u32>(), end_parts[1].parse::<u32>())
                && let Some(end_time) = NaiveTime::from_hms_opt(eh, em, 0)
                && event_time <= current_time
                && current_time < end_time
            {
                match best_current {
                    None => best_current = Some((i, event_time)),
                    Some((_, best_time)) if event_time > best_time => {
                        best_current = Some((i, event_time));
                    }
                    _ => {}
                }
            }
        }

        if first_next.is_none() && event_time > current_time {
            first_next = Some(i);
        }
    }

    if let Some((idx, _)) = best_current {
        Some((idx, true))
    } else {
        first_next.map(|idx| (idx, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{DisplayAttendee, AttendeeStatus, EventId};

    fn make_event_with_attendees(title: &str, attendees: Vec<DisplayAttendee>) -> DisplayEvent {
        DisplayEvent {
            id: EventId::Google { calendar_id: "test".to_string(), event_id: "test-id".to_string(), calendar_name: None, account_label: None },
            title: title.to_string(),
            time_str: "10:00".to_string(),
            end_time_str: None,
            date: NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
            accepted: true,
            is_organizer: false,
            is_free: false,
            meeting_url: None,
            description: None,
            location: None,
            attendees,
        }
    }

    #[test]
    fn test_event_matches_query_title() {
        let event = make_event_with_attendees("Sprint Planning", vec![]);
        assert!(event_matches_query(&event, "sprint"));
        assert!(event_matches_query(&event, "planning"));
    }

    #[test]
    fn test_event_matches_query_attendee_name() {
        let event = make_event_with_attendees("Meeting", vec![
            DisplayAttendee {
                name: Some("Alice Johnson".to_string()),
                email: "alice@example.com".to_string(),
                status: AttendeeStatus::Accepted,
            },
        ]);
        assert!(event_matches_query(&event, "alice"));
        assert!(event_matches_query(&event, "johnson"));
    }

    #[test]
    fn test_event_matches_query_attendee_email() {
        let event = make_event_with_attendees("Meeting", vec![
            DisplayAttendee {
                name: None,
                email: "bob@company.org".to_string(),
                status: AttendeeStatus::Accepted,
            },
        ]);
        assert!(event_matches_query(&event, "bob@company"));
        assert!(event_matches_query(&event, "company.org"));
    }

    #[test]
    fn test_event_matches_query_case_insensitive() {
        let event = make_event_with_attendees("Team Standup", vec![
            DisplayAttendee {
                name: Some("Charlie Brown".to_string()),
                email: "Charlie@Example.COM".to_string(),
                status: AttendeeStatus::Accepted,
            },
        ]);
        assert!(event_matches_query(&event, "team standup"));
        assert!(event_matches_query(&event, "charlie brown"));
        assert!(event_matches_query(&event, "charlie@example.com"));
    }

    #[test]
    fn test_event_match_type_title() {
        let event = make_event_with_attendees("Sprint Planning", vec![
            DisplayAttendee {
                name: Some("Alice".to_string()),
                email: "alice@example.com".to_string(),
                status: AttendeeStatus::Accepted,
            },
        ]);
        assert_eq!(event_match_type(&event, "sprint"), Some(MatchType::Title));
    }

    #[test]
    fn test_event_match_type_participant() {
        let event = make_event_with_attendees("Sprint Planning", vec![
            DisplayAttendee {
                name: Some("Alice Johnson".to_string()),
                email: "alice@example.com".to_string(),
                status: AttendeeStatus::Accepted,
            },
        ]);
        assert_eq!(event_match_type(&event, "alice"), Some(MatchType::Participant));
    }

    #[test]
    fn test_event_match_type_title_takes_priority() {
        // "Alice" appears in both title and attendees — title wins
        let event = make_event_with_attendees("Meeting with Alice", vec![
            DisplayAttendee {
                name: Some("Alice Johnson".to_string()),
                email: "alice@example.com".to_string(),
                status: AttendeeStatus::Accepted,
            },
        ]);
        assert_eq!(event_match_type(&event, "alice"), Some(MatchType::Title));
    }

    #[test]
    fn test_event_match_type_no_match() {
        let event = make_event_with_attendees("Sprint Planning", vec![
            DisplayAttendee {
                name: Some("Alice".to_string()),
                email: "alice@example.com".to_string(),
                status: AttendeeStatus::Accepted,
            },
        ]);
        assert_eq!(event_match_type(&event, "bob"), None);
    }

    #[test]
    fn test_event_matches_query_no_match() {
        let event = make_event_with_attendees("Sprint Planning", vec![
            DisplayAttendee {
                name: Some("Alice".to_string()),
                email: "alice@example.com".to_string(),
                status: AttendeeStatus::Accepted,
            },
        ]);
        assert!(!event_matches_query(&event, "retro"));
        assert!(!event_matches_query(&event, "bob"));
        assert!(!event_matches_query(&event, "xyz"));
    }
}
