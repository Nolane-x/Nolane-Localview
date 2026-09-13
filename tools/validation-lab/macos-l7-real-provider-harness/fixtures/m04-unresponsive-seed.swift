import AppKit
import Darwin
import Foundation

private let targetTitle = "LocalView M04 Target"
private let windowTitle = "LocalView M04 Seed"
private let stallDurationSeconds = 2.0

private final class SeedDelegate: NSObject, NSApplicationDelegate {
    private let stateURL: URL
    private let commandURL: URL
    private var window: NSWindow!
    private var button: NSButton!
    private var pressCount = 0
    private var stallCount = 0
    private var stalling = false
    private var timer: Timer?

    init(statePath: String, commandPath: String) {
        self.stateURL = URL(fileURLWithPath: statePath)
        self.commandURL = URL(fileURLWithPath: commandPath)
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        window = NSWindow(
            contentRect: NSRect(x: 140, y: 140, width: 520, height: 260),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: false
        )
        window.title = windowTitle
        window.isReleasedWhenClosed = false

        button = NSButton(frame: NSRect(x: 140, y: 95, width: 240, height: 52))
        button.title = targetTitle
        button.bezelStyle = .rounded
        button.target = self
        button.action = #selector(targetPressed(_:))
        window.contentView?.addSubview(button)

        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)

        timer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
            self?.pollCommand()
        }
        writeState()
    }

    @objc private func targetPressed(_ sender: Any?) {
        pressCount += 1
        writeState()
    }

    private func pollCommand() {
        guard FileManager.default.fileExists(atPath: commandURL.path) else {
            return
        }
        let command = (try? String(contentsOf: commandURL, encoding: .utf8))?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        try? FileManager.default.removeItem(at: commandURL)

        switch command {
        case "stall":
            stallCount += 1
            stalling = true
            writeState()

            // Deliberately block the AppKit main thread. The test harness uses
            // a short AX messaging timeout so a real remote AX request must
            // return kAXErrorCannotComplete while this state is independently
            // visible through the test-only state file.
            Thread.sleep(forTimeInterval: stallDurationSeconds)

            stalling = false
            writeState()
        case "quit":
            NSApp.terminate(nil)
        default:
            break
        }
    }

    private func writeState() {
        let record: [String: Any] = [
            "schema": "localview-v43-m04-seed-state-v1",
            "pid": Int(getpid()),
            "window_title": windowTitle,
            "target_title": targetTitle,
            "press_count": pressCount,
            "stall_count": stallCount,
            "stalling": stalling,
            "stall_duration_ms": Int(stallDurationSeconds * 1000.0),
        ]
        do {
            let data = try JSONSerialization.data(withJSONObject: record, options: [.sortedKeys])
            try data.write(to: stateURL, options: .atomic)
        } catch {
            fputs("M04 seed state write failed: \(error)\n", stderr)
        }
    }
}

guard let statePath = ProcessInfo.processInfo.environment["LOCALVIEW_M04_STATE_PATH"],
      let commandPath = ProcessInfo.processInfo.environment["LOCALVIEW_M04_COMMAND_PATH"] else {
    fputs("M04 seed requires LOCALVIEW_M04_STATE_PATH and LOCALVIEW_M04_COMMAND_PATH\n", stderr)
    exit(64)
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
private let delegate = SeedDelegate(statePath: statePath, commandPath: commandPath)
app.delegate = delegate
app.run()
