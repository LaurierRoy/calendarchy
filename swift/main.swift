import Carbon
import Cocoa

// Usage: calendarchy-hotkey <launch-command>
// Example: calendarchy-hotkey "open -na Ghostty.app --args -e /opt/homebrew/bin/calendarchy"
guard CommandLine.arguments.count >= 2 else {
    fputs("Usage: calendarchy-hotkey <launch-command>\n", stderr)
    exit(1)
}

let launchCommand = CommandLine.arguments[1]

// Helper to launch calendarchy
func launchCalendarchy() {
    let task = Process()
    task.launchPath = "/bin/bash"
    task.arguments = ["-c", launchCommand]
    try? task.run()
}

// --- Menu bar icon ---

let app = NSApplication.shared
app.setActivationPolicy(.accessory) // No Dock icon

class AppDelegate: NSObject, NSApplicationDelegate {
    var statusItem: NSStatusItem!

    func applicationDidFinishLaunching(_ notification: Notification) {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        if let button = statusItem.button {
            button.image = NSImage(systemSymbolName: "calendar", accessibilityDescription: "Calendarchy")
            button.action = #selector(handleClick(_:))
            button.sendAction(on: [.leftMouseUp, .rightMouseUp])
            button.target = self
        }
    }

    @objc func handleClick(_ sender: NSStatusBarButton) {
        let event = NSApp.currentEvent!
        if event.type == .rightMouseUp {
            // Right-click: show menu
            let menu = NSMenu()
            menu.addItem(NSMenuItem(title: "Open Calendarchy  \u{21e7}\u{2318}J", action: #selector(openCalendarchy), keyEquivalent: ""))
            menu.addItem(NSMenuItem.separator())
            menu.addItem(NSMenuItem(title: "Quit", action: #selector(quit), keyEquivalent: "q"))
            statusItem.menu = menu
            statusItem.button?.performClick(nil)
            statusItem.menu = nil // Reset so left-click works again
        } else {
            // Left-click: open calendarchy
            launchCalendarchy()
        }
    }

    @objc func openCalendarchy() {
        launchCalendarchy()
    }

    @objc func quit() {
        NSApplication.shared.terminate(nil)
    }
}

let delegate = AppDelegate()
app.delegate = delegate

// --- Global hotkey: Cmd+Shift+J ---

let hotKeyID = EventHotKeyID(signature: OSType(0x434C4452), id: 1)
var hotKeyRef: EventHotKeyRef?

let status = RegisterEventHotKey(
    UInt32(kVK_ANSI_J),
    UInt32(cmdKey | shiftKey),
    hotKeyID,
    GetApplicationEventTarget(),
    0,
    &hotKeyRef
)

if status != noErr {
    fputs("Failed to register hotkey (error \(status))\n", stderr)
}

var eventType = EventTypeSpec(
    eventClass: OSType(kEventClassKeyboard),
    eventKind: UInt32(kEventHotKeyPressed)
)

let callback: EventHandlerUPP = { _, event, userData -> OSStatus in
    launchCalendarchy()
    return noErr
}

var handlerRef: EventHandlerRef?
InstallEventHandler(
    GetApplicationEventTarget(),
    callback,
    1,
    &eventType,
    nil,
    &handlerRef
)

app.run()
