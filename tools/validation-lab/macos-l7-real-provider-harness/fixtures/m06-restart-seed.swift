import AppKit
import Darwin
import Foundation

private let windowTitle = "LocalView M06 Restart Seed"
private let launchMarker = DispatchTime.now().uptimeNanoseconds

private final class SeedDelegate: NSObject, NSApplicationDelegate {
    private let stateURL: URL
    private var window: NSWindow!

    init(statePath: String) {
        self.stateURL = URL(fileURLWithPath: statePath)
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        window = NSWindow(
            contentRect: NSRect(x: 220, y: 220, width: 520, height: 240),
            styleMask: [.titled, .closable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = windowTitle
        window.isReleasedWhenClosed = false

        let label = NSTextField(labelWithString: "M06 application-incarnation seed")
        label.frame = NSRect(x: 70, y: 100, width: 380, height: 28)
        label.alignment = .center
        window.contentView?.addSubview(label)

        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        writeState()
    }

    private func writeState() {
        let record: [String: Any] = [
            "schema": "localview-v43-m06-seed-state-v1",
            "pid": Int(getpid()),
            "launch_marker": launchMarker,
            "window_title": windowTitle,
            "ready": true,
        ]
        do {
            let data = try JSONSerialization.data(withJSONObject: record, options: [.sortedKeys])
            try data.write(to: stateURL, options: .atomic)
        } catch {
            fputs("M06 seed state write failed: \(error)\n", stderr)
        }
    }
}

guard let statePath = ProcessInfo.processInfo.environment["LOCALVIEW_M06_STATE_PATH"] else {
    fputs("M06 seed requires LOCALVIEW_M06_STATE_PATH\n", stderr)
    exit(64)
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
private let delegate = SeedDelegate(statePath: statePath)
app.delegate = delegate
app.run()
