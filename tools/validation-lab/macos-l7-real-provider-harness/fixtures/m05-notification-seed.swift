import AppKit
import Darwin
import Foundation

private let windowTitle = "LocalView M05 Notification Seed"

private final class SeedDelegate: NSObject, NSApplicationDelegate {
    private let stateURL: URL
    private var window: NSWindow!

    init(statePath: String) {
        self.stateURL = URL(fileURLWithPath: statePath)
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        window = NSWindow(
            contentRect: NSRect(x: 180, y: 180, width: 520, height: 240),
            styleMask: [.titled, .closable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = windowTitle
        window.isReleasedWhenClosed = false

        let label = NSTextField(labelWithString: "M05 AX observer registration seed")
        label.frame = NSRect(x: 80, y: 100, width: 360, height: 28)
        label.alignment = .center
        window.contentView?.addSubview(label)

        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        writeState()
    }

    private func writeState() {
        let record: [String: Any] = [
            "schema": "localview-v43-m05-seed-state-v1",
            "pid": Int(getpid()),
            "window_title": windowTitle,
            "ready": true,
        ]
        do {
            let data = try JSONSerialization.data(withJSONObject: record, options: [.sortedKeys])
            try data.write(to: stateURL, options: .atomic)
        } catch {
            fputs("M05 seed state write failed: \(error)\n", stderr)
        }
    }
}

guard let statePath = ProcessInfo.processInfo.environment["LOCALVIEW_M05_STATE_PATH"] else {
    fputs("M05 seed requires LOCALVIEW_M05_STATE_PATH\n", stderr)
    exit(64)
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
private let delegate = SeedDelegate(statePath: statePath)
app.delegate = delegate
app.run()
