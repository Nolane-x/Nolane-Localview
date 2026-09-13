import AppKit
import Darwin
import Foundation

private let baseTitle = "LocalView M07 RunLoop Seed"
private let launchMarker = DispatchTime.now().uptimeNanoseconds

private final class SeedDelegate: NSObject, NSApplicationDelegate {
    private let stateURL: URL
    private let commandURL: URL
    private var window: NSWindow!
    private var generation: Int = 0
    private var timer: Timer?

    init(statePath: String, commandPath: String) {
        self.stateURL = URL(fileURLWithPath: statePath)
        self.commandURL = URL(fileURLWithPath: commandPath)
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        window = NSWindow(
            contentRect: NSRect(x: 240, y: 240, width: 540, height: 250),
            styleMask: [.titled, .closable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = title(for: generation)
        window.isReleasedWhenClosed = false

        let label = NSTextField(labelWithString: "M07 AXObserver run-loop continuity seed")
        label.frame = NSRect(x: 70, y: 105, width: 400, height: 28)
        label.alignment = .center
        window.contentView?.addSubview(label)

        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        writeState()

        timer = Timer.scheduledTimer(withTimeInterval: 0.04, repeats: true) { [weak self] _ in
            self?.pollCommand()
        }
    }

    private func title(for generation: Int) -> String {
        "\(baseTitle) \(generation)"
    }

    private func pollCommand() {
        guard let command = try? String(contentsOf: commandURL, encoding: .utf8)
            .trimmingCharacters(in: .whitespacesAndNewlines),
              !command.isEmpty else {
            return
        }

        try? "".write(to: commandURL, atomically: true, encoding: .utf8)

        let parts = command.split(separator: " ")
        guard parts.count == 2,
              parts[0] == "title",
              let requestedGeneration = Int(parts[1]),
              requestedGeneration > generation else {
            return
        }

        generation = requestedGeneration
        window.title = title(for: generation)
        writeState()
    }

    private func writeState() {
        let record: [String: Any] = [
            "schema": "localview-v43-m07-seed-state-v1",
            "pid": Int(getpid()),
            "launch_marker": launchMarker,
            "title_generation": generation,
            "window_title": window.title,
            "ready": true,
        ]
        do {
            let data = try JSONSerialization.data(withJSONObject: record, options: [.sortedKeys])
            try data.write(to: stateURL, options: .atomic)
        } catch {
            fputs("M07 seed state write failed: \(error)\n", stderr)
        }
    }
}

guard let statePath = ProcessInfo.processInfo.environment["LOCALVIEW_M07_STATE_PATH"],
      let commandPath = ProcessInfo.processInfo.environment["LOCALVIEW_M07_COMMAND_PATH"] else {
    fputs("M07 seed requires LOCALVIEW_M07_STATE_PATH and LOCALVIEW_M07_COMMAND_PATH\n", stderr)
    exit(64)
}

try? "".write(toFile: commandPath, atomically: true, encoding: .utf8)
let app = NSApplication.shared
app.setActivationPolicy(.regular)
private let delegate = SeedDelegate(statePath: statePath, commandPath: commandPath)
app.delegate = delegate
app.run()
