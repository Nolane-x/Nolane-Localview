from pathlib import Path

path = Path("crates/windows-observe-runtime/tests/windows_runtime_dispatch_smoke.rs")
text = path.read_text()
old = "smoke_parent_wndproc as isize"
new = "smoke_parent_wndproc as *const () as isize"
if text.count(old) != 1:
    raise SystemExit(f"expected exactly one function-to-integer cast, found {text.count(old)}")
path.write_text(text.replace(old, new, 1))
