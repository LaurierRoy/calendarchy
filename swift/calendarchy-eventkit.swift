import EventKit
import Foundation

// calendarchy-eventkit: Fetch calendar events via EventKit and output as JSON
// Usage: calendarchy-eventkit <start-date> <end-date>
// Dates in YYYY-MM-DD format
// Outputs JSON array of events to stdout

guard CommandLine.arguments.count >= 3 else {
    fputs("Usage: calendarchy-eventkit <start-date> <end-date>\n", stderr)
    exit(1)
}

let startArg = CommandLine.arguments[1]
let endArg = CommandLine.arguments[2]

let dateFmt = DateFormatter()
dateFmt.dateFormat = "yyyy-MM-dd"
dateFmt.timeZone = TimeZone.current

guard let startDate = dateFmt.date(from: startArg),
      let endDate = Calendar.current.date(byAdding: .day, value: 1, to: dateFmt.date(from: endArg)!) else {
    fputs("Invalid date format. Use YYYY-MM-DD\n", stderr)
    exit(1)
}

let store = EKEventStore()

let semaphore = DispatchSemaphore(value: 0)
var accessGranted = false

if #available(macOS 14.0, *) {
    store.requestFullAccessToEvents { granted, error in
        accessGranted = granted
        if let error = error {
            fputs("Access error: \(error.localizedDescription)\n", stderr)
        }
        semaphore.signal()
    }
} else {
    store.requestAccess(to: .event) { granted, error in
        accessGranted = granted
        if let error = error {
            fputs("Access error: \(error.localizedDescription)\n", stderr)
        }
        semaphore.signal()
    }
}

semaphore.wait()

guard accessGranted else {
    fputs("Calendar access denied. Grant access in System Settings > Privacy & Security > Calendars.\n", stderr)
    // Output empty array so the caller can distinguish "no access" from "no events"
    print("[]")
    exit(1)
}

let predicate = store.predicateForEvents(withStart: startDate, end: endDate, calendars: nil)
let events = store.events(matching: predicate)

let timeFmt = DateFormatter()
timeFmt.dateFormat = "HH:mm"
timeFmt.timeZone = TimeZone.current

let dayFmt = DateFormatter()
dayFmt.dateFormat = "yyyy-MM-dd"
dayFmt.timeZone = TimeZone.current

var result: [[String: Any]] = []

for event in events {
    var dict: [String: Any] = [
        "title": event.title ?? "(No title)",
        "date": dayFmt.string(from: event.startDate),
        "all_day": event.isAllDay,
        "calendar_name": event.calendar.title,
        "calendar_type": calendarSourceType(event.calendar.source.sourceType),
        "accepted": attendeeStatus(event) != "declined",
        "is_organizer": event.organizer?.isCurrentUser ?? false,
        "is_free": event.availability == .free,
    ]

    if !event.isAllDay {
        dict["start_time"] = timeFmt.string(from: event.startDate)
        dict["end_time"] = timeFmt.string(from: event.endDate)
    }

    if let location = event.location, !location.isEmpty {
        dict["location"] = location
    }

    if let notes = event.notes, !notes.isEmpty {
        dict["description"] = notes
    }

    if let url = event.url {
        dict["url"] = url.absoluteString
    }

    // Attendees
    var attendees: [[String: String]] = []
    if let participants = event.attendees {
        for p in participants {
            var a: [String: String] = [:]
            if let name = p.name {
                a["name"] = name
            }
            let emailStr = p.url.absoluteString.replacingOccurrences(of: "mailto:", with: "")
            a["email"] = emailStr.isEmpty ? (p.name ?? "unknown") : emailStr
            a["status"] = participantStatus(p.participantStatus)
            if p.isCurrentUser {
                a["is_self"] = "true"
            }
            attendees.append(a)
        }
    }
    if !attendees.isEmpty {
        dict["attendees"] = attendees
    }

    // Meeting URL: check URL field, location, and notes for common patterns
    let meetingUrl = extractMeetingUrl(event)
    if let meetingUrl = meetingUrl {
        dict["meeting_url"] = meetingUrl
    }

    result.append(dict)
}

// Output as JSON
let jsonData = try! JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
print(String(data: jsonData, encoding: .utf8)!)

// --- Helpers ---

func calendarSourceType(_ type: EKSourceType) -> String {
    switch type {
    case .local: return "local"
    case .exchange: return "exchange"
    case .calDAV: return "caldav"
    case .subscribed: return "subscribed"
    case .birthdays: return "birthdays"
    case .mobileMe: return "mobileme"
    @unknown default: return "unknown"
    }
}

func participantStatus(_ status: EKParticipantStatus) -> String {
    switch status {
    case .accepted: return "accepted"
    case .declined: return "declined"
    case .tentative: return "tentative"
    case .unknown: return "needs_action"
    case .pending: return "needs_action"
    case .delegated: return "accepted"
    case .completed: return "accepted"
    case .inProcess: return "accepted"
    @unknown default: return "needs_action"
    }
}

func attendeeStatus(_ event: EKEvent) -> String {
    guard let attendees = event.attendees else { return "accepted" }
    for a in attendees {
        if a.isCurrentUser {
            return participantStatus(a.participantStatus)
        }
    }
    return "accepted"
}

func extractMeetingUrl(_ event: EKEvent) -> String? {
    // Check URL field
    if let url = event.url?.absoluteString, isMeetingUrl(url) {
        return url
    }
    // Check location
    if let loc = event.location, let url = findMeetingUrl(in: loc) {
        return url
    }
    // Check notes
    if let notes = event.notes, let url = findMeetingUrl(in: notes) {
        return url
    }
    return nil
}

func isMeetingUrl(_ url: String) -> Bool {
    let patterns = ["zoom.us/j/", "zoom.us/my/", "meet.google.com/", "teams.microsoft.com/"]
    return patterns.contains(where: { url.contains($0) })
}

func findMeetingUrl(in text: String) -> String? {
    let pattern = #"https?://[^\s<>\"\')]*(zoom\.us/[^\s<>\"\')]*|meet\.google\.com/[^\s<>\"\')]*|teams\.microsoft\.com/[^\s<>\"\')]*)"#
    guard let regex = try? NSRegularExpression(pattern: pattern, options: []),
          let match = regex.firstMatch(in: text, options: [], range: NSRange(text.startIndex..., in: text)),
          let range = Range(match.range, in: text) else {
        return nil
    }
    return String(text[range])
}
