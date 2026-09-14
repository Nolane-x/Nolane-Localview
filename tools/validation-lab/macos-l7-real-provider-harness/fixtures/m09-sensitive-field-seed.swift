import AppKit
import Darwin
import Foundation

private let windowTitle = "LocalView M09 Sensitive Field Seed"

private final class CountingSecureTextField: NSSecureTextField {
    private let stateURL: URL
    private let stateLock = NSLock()
    private var valueReadCountStorage = 0

    init(frame: NSRect, stateURL: URL) {
        self.stateURL = stateURL
        super.init(frame: frame)
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func accessibilityValue() -> String? {
        stateLock.lock()
        valueReadCountStorage += 1
        stateLock.unlock()
        writeState()
        return super.accessibilityValue()
    }

    func writeState() {
        stateLock.lock()
        let count = valueReadCountStorage
        stateLock.unlock()
        let record: [String: Any] = [
            "schema": "localview-v43-m09-seed-state-v1",
            "pid": Int(getpid()),
            "value_read_count": count,
        ]
        do {
            let data = try JSONSerialization.data(withJSONObject: record, options: [.sortedKeys])
            try data.write(to: stateURL, options: .atomic)
        } catch {
            fputs("M09 seed state write failed: \(error)\n", stderr)
        }
    }
}

private final class SeedDelegate: NSObject, NSApplicationDelegate {
    private let stateURL: URL
    private let secret: String
    private var window: NSWindow!
    private var secureField: CountingSecureTextField!

    init(statePath: String, secret: String) {
        self.stateURL = URL(fileURLWithPath: statePath)
        self.secret = secret
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        window = NSWindow(
            contentRect: NSRect(x: 160, y: 160, width: 560, height: 260),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: false
        )
        window.title = windowTitle
        window.isReleasedWhenClosed = false

        let label = NSTextField(labelWithString: "Protected credential")
        label.frame = NSRect(x: 80, y: 155, width: 400, height: 28)
        window.contentView?.addSubview(label)

        secureField = CountingSecureTextField(
            frame: NSRect(x: 80, y: 100, width: 400, height: 32),
            stateURL: stateURL
        )
        secureField.stringValue = secret
        secureField.setAccessibilityLabel("LocalView M09 protected credential")
        window.contentView?.addSubview(secureField)

        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        secureField.writeState()
    }
}

guard let statePath = ProcessInfo.processInfo.environment["LOCALVIEW_M09_STATE_PATH"],
      let secret = ProcessInfo.processInfo.environment["LOCALVIEW_M09_SECRET"] else {
    fputs("M09 seed requires LOCALVIEW_M09_STATE_PATH and LOCALVIEW_M09_SECRET\n", stderr)
    exit(64)
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
private let delegate = SeedDelegate(statePath: statePath, secret: secret)
app.delegate = delegate
app.run()
