import Carbon
import Cocoa

// Usage: calendarchy-hotkey <launch-command>
// Example: calendarchy-hotkey "open -na Ghostty.app --args -e /opt/homebrew/bin/calendarchy"
guard CommandLine.arguments.count >= 2 else {
    fputs("Usage: calendarchy-hotkey <launch-command>\n", stderr)
    exit(1)
}

let launchCommand = CommandLine.arguments[1]

// Hotkey: Cmd+Shift+J
// Key code 0x26 = J, modifiers: cmdKey (0x100) + shiftKey (0x200)
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

guard status == noErr else {
    fputs("Failed to register hotkey (error \(status))\n", stderr)
    exit(1)
}

// Install event handler for hotkey press
var eventType = EventTypeSpec(
    eventClass: OSType(kEventClassKeyboard),
    eventKind: UInt32(kEventHotKeyPressed)
)

let callback: EventHandlerUPP = { _, event, userData -> OSStatus in
    guard let userData = userData else { return OSStatus(eventNotHandledErr) }
    let command = Unmanaged<NSString>.fromOpaque(userData).takeUnretainedValue() as String
    let task = Process()
    task.launchPath = "/bin/bash"
    task.arguments = ["-c", command]
    try? task.run()
    return noErr
}

let commandRef = Unmanaged.passRetained(launchCommand as NSString).toOpaque()

var handlerRef: EventHandlerRef?
InstallEventHandler(
    GetApplicationEventTarget(),
    callback,
    1,
    &eventType,
    commandRef,
    &handlerRef
)

NSApplication.shared.run()
