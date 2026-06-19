use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

/// Attendee information for display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayAttendee {
    pub name: Option<String>,  // Display name if available
    pub email: String,
    pub status: AttendeeStatus,
}

/// Attendee response status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AttendeeStatus {
    Accepted,
    Declined,
    Tentative,
    NeedsAction,
    Organizer,
}

impl AttendeeStatus {
    /// Get the display icon for this status
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Accepted | Self::Organizer => "\u{2713}", // ✓
            Self::Declined => "\u{2717}",                   // ✗
            Self::Tentative | Self::NeedsAction => "?",
        }
    }

    /// Get the display color for this status
    pub fn color(&self) -> crossterm::style::Color {
        use crossterm::style::Color;
        match self {
            Self::Accepted => Color::Green,
            Self::Organizer => Color::Blue,
            Self::Declined => Color::Red,
            Self::Tentative => Color::Yellow,
            Self::NeedsAction => Color::DarkGrey,
        }
    }
}

/// Event identifier for API actions (accept/decline/delete)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventId {
    /// Google Calendar event (calendar_id, event_id, calendar_name for display)
    Google { calendar_id: String, event_id: String, calendar_name: Option<String>, account_label: Option<String> },
    /// iCloud CalDAV event (calendar_url, event_uid, etag for updates, calendar_name for display)
    ICloud { calendar_url: String, event_uid: String, etag: Option<String>, calendar_name: Option<String>, account_label: Option<String> },
}

/// Unified event representation for display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayEvent {
    pub id: EventId,
    pub title: String,
    pub time_str: String,
    pub end_time_str: Option<String>,
    pub date: NaiveDate,
    pub accepted: bool, // true if accepted or organizer, false if declined/tentative/needs-action
    pub is_organizer: bool, // true if the user created/organizes this event
    #[serde(default)] // backwards compat with old cache
    pub is_free: bool, // true if event is marked as "free" (doesn't block time)
    pub meeting_url: Option<String>, // Zoom, Meet, Teams link if available
    pub description: Option<String>,
    pub location: Option<String>,
    pub attendees: Vec<DisplayAttendee>,
}

/// Serializable cache format for disk persistence
#[derive(Serialize, Deserialize)]
struct DiskCache {
    sources: Vec<HashMap<NaiveDate, Vec<DisplayEvent>>>,
}

/// Source-specific event cache
pub struct SourceCache {
    by_date: HashMap<NaiveDate, Vec<DisplayEvent>>,
    fetched_months: HashSet<(i32, u32)>,
}

impl SourceCache {
    pub fn new() -> Self {
        Self {
            by_date: HashMap::new(),
            fetched_months: HashSet::new(),
        }
    }

    pub fn has_month(&self, date: NaiveDate) -> bool {
        self.fetched_months.contains(&(date.year(), date.month()))
    }

    pub fn store(&mut self, events: Vec<DisplayEvent>, month_date: NaiveDate) {
        // Clear existing events for this month before storing fresh data
        let year = month_date.year();
        let month = month_date.month();
        self.by_date.retain(|date, _| date.year() != year || date.month() != month);

        for event in events {
            self.by_date
                .entry(event.date)
                .or_default()
                .push(event);
        }
        self.fetched_months.insert((year, month));
    }

    pub fn get(&self, date: NaiveDate) -> &[DisplayEvent] {
        self.by_date
            .get(&date)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn has_events(&self, date: NaiveDate) -> bool {
        self.by_date
            .get(&date)
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }

    pub fn all_events(&self) -> impl Iterator<Item = &DisplayEvent> {
        self.by_date.values().flat_map(|v| v.iter())
    }

    pub fn clear(&mut self) {
        self.by_date.clear();
        self.fetched_months.clear();
    }

    /// Get raw data for serialization
    pub fn raw_data(&self) -> &HashMap<NaiveDate, Vec<DisplayEvent>> {
        &self.by_date
    }

    /// Load from raw data (for cache restore)
    pub fn load_from(&mut self, data: HashMap<NaiveDate, Vec<DisplayEvent>>) {
        self.by_date = data;
        // Don't mark months as fetched - we want to refresh from network
    }
}

impl Default for SourceCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Combined event cache for all sources
pub struct EventCache {
    pub sources: Vec<SourceCache>,
}

impl EventCache {
    pub fn new(num_sources: usize) -> Self {
        Self {
            sources: (0..num_sources).map(|_| SourceCache::new()).collect(),
        }
    }

    /// Check if any source has events on this date
    pub fn has_events(&self, date: NaiveDate) -> bool {
        self.sources.iter().any(|s| s.has_events(date))
    }

    /// Clear all caches
    pub fn clear(&mut self) {
        self.sources.iter_mut().for_each(|s| s.clear());
    }

    /// Get cache file path
    fn cache_path() -> Option<PathBuf> {
        dirs::cache_dir().map(|p| p.join("calendarchy").join("events.json"))
    }

    /// Save cache to disk
    pub fn save_to_disk(&self) {
        let Some(path) = Self::cache_path() else { return };

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let cache = DiskCache {
            sources: self.sources.iter().map(|s| s.raw_data().clone()).collect(),
        };

        if let Ok(json) = serde_json::to_string(&cache) {
            let _ = fs::write(&path, json);
        }
    }

    /// Load cache from disk
    pub fn load_from_disk(&mut self) -> bool {
        let Some(path) = Self::cache_path() else { return false };

        let Ok(json) = fs::read_to_string(&path) else { return false };

        // Try old format first (google/icloud keys)
        if let Ok(old) = serde_json::from_str::<OldDiskCache>(&json) {
            if self.sources.len() >= 2 {
                self.sources[0].load_from(old.google);
                if self.sources.len() >= 2 {
                    self.sources[1].load_from(old.icloud);
                }
            }
            return true;
        }

        // New format
        let Ok(cache) = serde_json::from_str::<DiskCache>(&json) else { return false };
        for (i, data) in cache.sources.into_iter().enumerate() {
            if i < self.sources.len() {
                self.sources[i].load_from(data);
            }
        }
        true
    }
}

impl Default for EventCache {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Old cache format for backward compatibility
#[derive(Serialize, Deserialize)]
struct OldDiskCache {
    google: HashMap<NaiveDate, Vec<DisplayEvent>>,
    #[serde(default)]
    icloud: HashMap<NaiveDate, Vec<DisplayEvent>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(title: &str, date: NaiveDate, time: &str) -> DisplayEvent {
        DisplayEvent {
            id: EventId::Google { calendar_id: "test".to_string(), event_id: "test-id".to_string(), calendar_name: None, account_label: None },
            title: title.to_string(),
            time_str: time.to_string(),
            end_time_str: None,
            date,
            accepted: true,
            is_organizer: false,
            is_free: false,
            meeting_url: None,
            description: None,
            location: None,
            attendees: vec![],
        }
    }

    #[test]
    fn test_source_cache_store_and_get() {
        let mut cache = SourceCache::new();
        let date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let month_date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        let events = vec![
            make_event("Meeting 1", date, "10:00"),
            make_event("Meeting 2", date, "14:00"),
        ];

        cache.store(events, month_date);

        let retrieved = cache.get(date);
        assert_eq!(retrieved.len(), 2);
        assert_eq!(retrieved[0].title, "Meeting 1");
        assert_eq!(retrieved[1].title, "Meeting 2");
    }

    #[test]
    fn test_source_cache_has_month() {
        let mut cache = SourceCache::new();
        let month_date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        assert!(!cache.has_month(month_date));

        cache.store(vec![], month_date);

        assert!(cache.has_month(month_date));
        assert!(!cache.has_month(NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()));
    }

    #[test]
    fn test_source_cache_store_replaces_month_data() {
        let mut cache = SourceCache::new();
        let date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let month_date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        cache.store(vec![make_event("Old Event", date, "09:00")], month_date);
        assert_eq!(cache.get(date).len(), 1);

        cache.store(vec![make_event("New Event", date, "10:00")], month_date);
        assert_eq!(cache.get(date).len(), 1);
        assert_eq!(cache.get(date)[0].title, "New Event");
    }

    #[test]
    fn test_source_cache_has_events() {
        let mut cache = SourceCache::new();
        let date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let empty_date = NaiveDate::from_ymd_opt(2026, 1, 16).unwrap();
        let month_date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        cache.store(vec![make_event("Event", date, "10:00")], month_date);

        assert!(cache.has_events(date));
        assert!(!cache.has_events(empty_date));
    }

    #[test]
    fn test_source_cache_clear() {
        let mut cache = SourceCache::new();
        let date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let month_date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        cache.store(vec![make_event("Event", date, "10:00")], month_date);
        assert!(cache.has_month(month_date));

        cache.clear();
        assert!(!cache.has_month(month_date));
    }

    #[test]
    fn test_source_cache_load_from_does_not_mark_fetched() {
        let mut cache = SourceCache::new();
        let date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let month_date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        let mut data = HashMap::new();
        data.insert(date, vec![make_event("Cached Event", date, "10:00")]);

        cache.load_from(data);

        assert_eq!(cache.get(date).len(), 1);
        assert!(!cache.has_month(month_date));
    }

    #[test]
    fn test_event_cache_has_events() {
        let mut cache = EventCache::new(2);
        let date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let month_date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        assert!(!cache.has_events(date));

        cache.sources[0].store(vec![make_event("Event 1", date, "10:00")], month_date);
        assert!(cache.has_events(date));

        cache.sources[0].clear();
        assert!(!cache.has_events(date));

        cache.sources[1].store(vec![make_event("Event 2", date, "11:00")], month_date);
        assert!(cache.has_events(date));
    }

    #[test]
    fn test_display_event_serialization() {
        let event = make_event("Test Meeting", NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(), "14:30");

        let json = serde_json::to_string(&event).unwrap();
        let parsed: DisplayEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.title, "Test Meeting");
        assert_eq!(parsed.time_str, "14:30");
        assert!(parsed.accepted);
    }
}
