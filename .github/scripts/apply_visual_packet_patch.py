from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, found {count}")
    file.write_text(text.replace(old, new, 1), encoding="utf-8")


visual = Path("apps/desktop/src-tauri/src/visual_capture.rs")
visual_text = visual.read_text(encoding="utf-8")
if 'include!("visual_packet_impl.rs");' not in visual_text:
    if "async fn capture_redacted_viewport_after_gate(" not in visual_text:
        raise SystemExit("shared redacted capture authority is missing")
    if "pub async fn capture_progressive_target(" not in visual_text:
        raise SystemExit("progressive target foundation is missing")
    visual.write_text(
        visual_text.rstrip() + '\n\ninclude!("visual_packet_impl.rs");\n',
        encoding="utf-8",
    )

replace_once(
    "apps/desktop/src-tauri/src/lib.rs",
    "            visual_capture::capture_progressive_target,\n",
    "            visual_capture::capture_progressive_target,\n"
    "            visual_capture::capture_visual_packet,\n",
)
