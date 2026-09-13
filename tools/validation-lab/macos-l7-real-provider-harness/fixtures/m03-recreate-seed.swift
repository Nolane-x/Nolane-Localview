import AppKit
import Darwin
import Foundation

private let targetTitle = "LocalView M03 Target"
private let windowTitle = "LocalView M03 Seed"

private final class SeedDelegate: NSObject, NSApplicationDelegate {
    private let stateURL: URL
    private let commandURL: URL
    private var window: NSWindow!
    private var button: NSButton?
    private var generation = 0
    private var pressCount = 0
    private var timer: Timer?

    init(statePath: String, commandPath: String) {
        self.stateURL = URL(fileURLWithPath: statePath)
        self.commandURL = URL(fileURLWithPath: commandPath)
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        window = NSWindow(
            contentRect: NSRect(x: 120, y: 120, width: 520, height: 260),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: false
        )
        window.title = windowTitle
        window.isReleasedWhenClosed = false
        recreateButton()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)

        timer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
            self?.pollCommand()
        }
        writeState()
    }

    private func recreateButton() {
        button?.removeFromSuperview()
        button = nil

        autoreleasepool {
            generation += 1
            let replacement = NSButton(frame: NSRect(x: 140, y: 95, width: 240, height: 52))
            replacement.title = targetTitle
            replacement.bezelStyle = .rounded
            replacement.target = self
            replacement.action = #selector(targetPressed(_:))
            window.contentView?.addSubview(replacement)
            button = replacement
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
        case "recreate":
            recreateButton()
        case "quit":
            NSApp.terminate(nil)
        default:
            break
        }
    }

    private func writeState() {
        let record: [String: Any] = [
            "schema": "localview-v43-m03-seed-state-v1",
            "pid": Int(getpid()),
            "window_title": windowTitle,
            "target_title": targetTitle,
            "element_generation": generation,
            "press_count": pressCount,
        ]
        do {
            let data = try JSONSerialization.data(withJSONObject: record, options: [.sortedKeys])
            try data.write(to: stateURL, options: .atomic)
        } catch {
            fputs("M03 seed state write failed: \(error)\n", stderr)
        }
    }
}

guard let statePath = ProcessInfo.processInfo.environment["LOCALVIEW_M03_STATE_PATH"],
      let commandPath = ProcessInfo.processInfo.environment["LOCALVIEW_M03_COMMAND_PATH"] else {
    fputs("M03 seed requires LOCALVIEW_M03_STATE_PATH and LOCALVIEW_M03_COMMAND_PATH\n", stderr)
    exit(64)
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
let delegate = SeedDelegate(statePath: statePath, commandPath: commandPath)
app.delegate = delegate
app.run()
