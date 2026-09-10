from pathlib import Path
import subprocess

path = Path("crates/control/src/windows_consequential.rs")
source = path.read_text()

replacements = [
    (
        """struct WindowsConsequentialControlHandle {\n    journal: Arc<ConsequentialJournal>,\n    pending: Arc<Mutex<HashMap<Uuid, PendingWindowsConsequentialPlan>>>,\n    plan_gate: Arc<Mutex<()>>,\n}""",
        """struct WindowsConsequentialControlHandle {\n    journal: Arc<ConsequentialJournal>,\n    pending: Arc<Mutex<HashMap<Uuid, PendingWindowsConsequentialPlan>>>,\n    plan_gate: Arc<Mutex<()>>,\n    set_value: set_value_http::WindowsSetValuePayloadAuthority,\n}""",
    ),
    (
        """    match journal {\n        Some(journal) => {\n            entries.insert(""",
        """    match journal {\n        Some(journal) => {\n            let set_value = match set_value_http::WindowsSetValuePayloadAuthority::new() {\n                Ok(authority) => authority,\n                Err(_) => {\n                    entries.remove(&key);\n                    return;\n                }\n            };\n            entries.insert(""",
    ),
    (
        """                        journal,\n                        pending: Arc::new(Mutex::new(HashMap::new())),\n                        plan_gate: Arc::new(Mutex::new(())),\n                    },""",
        """                        journal,\n                        pending: Arc::new(Mutex::new(HashMap::new())),\n                        plan_gate: Arc::new(Mutex::new(())),\n                        set_value,\n                    },""",
    ),
    (
        """    handle\n        .pending\n        .lock()\n        .await\n        .retain(|_, plan| plan.queued.action.session_id != session_id);\n}""",
        """    handle\n        .pending\n        .lock()\n        .await\n        .retain(|_, plan| plan.queued.action.session_id != session_id);\n    handle.set_value.release_session(session_id).await;\n}""",
    ),
    (
        """            WindowsConsequentialControlHandle {\n                journal,\n                pending: Arc::new(Mutex::new(HashMap::new())),\n                plan_gate: Arc::new(Mutex::new(())),\n            },""",
        """            WindowsConsequentialControlHandle {\n                journal,\n                pending: Arc::new(Mutex::new(HashMap::new())),\n                plan_gate: Arc::new(Mutex::new(())),\n                set_value: set_value_http::WindowsSetValuePayloadAuthority::new().unwrap(),\n            },""",
    ),
]

for old, new in replacements:
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"expected exactly one match, found {count}: {old[:100]!r}")
    source = source.replace(old, new, 1)

path.write_text(source)

set_value_path = Path("crates/control/src/windows_consequential/set_value_http.rs")
set_value_source = set_value_path.read_text()
stale_expectations = [
    """#[cfg_attr(\n    not(test),\n    expect(\n        dead_code,\n        reason = \"Task 8 Stage 2 payload authority is intentionally introduced before Stage 3 server-owned route wiring\"\n    )\n)]\nstruct ProcessLocalSetValuePayload""",
    """#[cfg_attr(\n    not(test),\n    expect(\n        dead_code,\n        reason = \"Task 8 Stage 2 pending payload authority is intentionally introduced before Stage 3 confirmation wiring\"\n    )\n)]\nstruct PendingWindowsSetValuePayload""",
]
for old in stale_expectations:
    replacement = old.split(")]\n", 1)[1]
    count = set_value_source.count(old)
    if count != 1:
        raise SystemExit(f"expected exactly one stale expectation, found {count}: {old[-80:]!r}")
    set_value_source = set_value_source.replace(old, replacement, 1)
set_value_path.write_text(set_value_source)

# The runner's later path-limited git add intentionally omits this file. Stage it
# here so the verified commit contains the annotation retirement atomically with
# the control-lifetime wiring that makes those expectations stale.
subprocess.run(
    ["git", "add", "--", "crates/control/src/windows_consequential/set_value_http.rs"],
    check=True,
)
