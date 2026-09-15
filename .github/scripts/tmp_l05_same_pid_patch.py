from pathlib import Path

seed_path = Path("tools/provider-seeds/linux-atspi-bus-reconnect-seed/seed.c")
seed = seed_path.read_text()
old = "#include <atk/atk.h>\n#include <gtk/gtk.h>\n"
new = "#include <atk/atk.h>\n#include <gtk/gtk.h>\n#include <gmodule.h>\n"
if seed.count(old) != 1:
    raise SystemExit("unexpected seed include anchor")
seed = seed.replace(old, new)

anchor = '''static void emit_error(const char *command) {
  fprintf(stdout, "{\\\"event\\\":\\\"error\\\",\\\"command\\\":\\\"%s\\\"}\\n", command);
  fflush(stdout);
}

'''
insertion = '''static void emit_error(const char *command) {
  fprintf(stdout, "{\\\"event\\\":\\\"error\\\",\\\"command\\\":\\\"%s\\\"}\\n", command);
  fflush(stdout);
}

static void emit_bridge_reinitialized(void) {
  puts("{\\\"event\\\":\\\"bridge_reinitialized\\\"}");
  fflush(stdout);
}

typedef int (*AtkBridgeInitFunc)(int *, char ***);
typedef void (*AtkBridgeCleanupFunc)(void);

static gboolean reinitialize_atk_bridge(void) {
  GModule *module;
  AtkBridgeInitFunc init_func = NULL;
  AtkBridgeCleanupFunc cleanup_func = NULL;
  int result;

  module = g_module_open("libatk-bridge-2.0.so.0", G_MODULE_BIND_LAZY);
  if (module == NULL) {
    fprintf(stderr, "failed to open libatk-bridge-2.0.so.0: %s\\n", g_module_error());
    return FALSE;
  }

  if (!g_module_symbol(module, "atk_bridge_adaptor_cleanup", (gpointer *)&cleanup_func) ||
      cleanup_func == NULL) {
    fprintf(stderr, "failed to resolve atk_bridge_adaptor_cleanup: %s\\n", g_module_error());
    g_module_close(module);
    return FALSE;
  }
  if (!g_module_symbol(module, "atk_bridge_adaptor_init", (gpointer *)&init_func) ||
      init_func == NULL) {
    fprintf(stderr, "failed to resolve atk_bridge_adaptor_init: %s\\n", g_module_error());
    g_module_close(module);
    return FALSE;
  }

  cleanup_func();
  result = init_func(NULL, NULL);
  g_module_close(module);
  if (result != 0) {
    fprintf(stderr, "atk_bridge_adaptor_init failed after bus replacement: %d\\n", result);
    return FALSE;
  }

  return TRUE;
}

'''
if seed.count(anchor) != 1:
    raise SystemExit("unexpected seed emit_error anchor")
seed = seed.replace(anchor, insertion)

old = '''  if (strcmp(line, "status") == 0) {
    emit_status();
  } else if (strcmp(line, "quit") == 0) {
'''
new = '''  if (strcmp(line, "status") == 0) {
    emit_status();
  } else if (strcmp(line, "reinitialize_bridge") == 0) {
    if (reinitialize_atk_bridge()) {
      emit_bridge_reinitialized();
    } else {
      emit_error(line);
    }
  } else if (strcmp(line, "quit") == 0) {
'''
if seed.count(old) != 1:
    raise SystemExit("unexpected seed command anchor")
seed_path.write_text(seed.replace(old, new))

harness_path = Path("tools/validation-lab/linux-l7-real-provider-harness/tests/v43_real_provider_l05.rs")
harness = harness_path.read_text()
old = '''        let fresh_observer = connect_replacement_observer().await;
        let fresh_object = find_accessible_by_name(&fresh_observer, ACCESSIBLE_NAME).await;
'''
new = '''        let fresh_observer = connect_replacement_observer().await;
        let bridge_reinitialized = seed.command("reinitialize_bridge");
        assert_eq!(
            bridge_reinitialized["event"], "bridge_reinitialized",
            "Ubuntu 24.04 at-spi2-core 2.52 requires the live toolkit bridge to reconnect explicitly after accessibility-bus replacement"
        );
        let toolkit_bridge_reinitialized_same_pid = seed.is_running() && seed.pid() == target_pid;
        assert!(
            toolkit_bridge_reinitialized_same_pid,
            "ATK bridge recovery must occur inside the original GTK target process"
        );
        let fresh_object = find_accessible_by_name(&fresh_observer, ACCESSIBLE_NAME).await;
'''
if harness.count(old) != 1:
    raise SystemExit("unexpected fresh observer anchor")
harness = harness.replace(old, new)
harness = harness.replace(
    "        let mut provider = LinuxAtspiProvider::connect(\n",
    "        let provider = LinuxAtspiProvider::connect(\n",
    1,
)
old = '''            "target_process_survived_bus_restart": target_process_survived_bus_restart,
            "target_pid": target_pid,
'''
new = '''            "target_process_survived_bus_restart": target_process_survived_bus_restart,
            "toolkit_bridge_reinitialized_same_pid": toolkit_bridge_reinitialized_same_pid,
            "toolkit_recovery_mode": "same_pid_atk_bridge_cleanup_init",
            "target_pid": target_pid,
'''
if harness.count(old) != 1:
    raise SystemExit("unexpected evidence anchor")
harness_path.write_text(harness.replace(old, new))

workflow_path = Path(".github/workflows/v43-l05-real-provider.yml")
workflow = workflow_path.read_text()
old = "            $(pkg-config --cflags --libs gtk+-3.0 atk)\n"
new = "            $(pkg-config --cflags --libs gtk+-3.0 atk gmodule-2.0)\n"
if workflow.count(old) != 1:
    raise SystemExit("unexpected seed compile anchor")
workflow = workflow.replace(old, new)
old = '              "target_process_survived_bus_restart": True,\n'
new = '              "target_process_survived_bus_restart": True,\n              "toolkit_bridge_reinitialized_same_pid": True,\n              "toolkit_recovery_mode": "same_pid_atk_bridge_cleanup_init",\n'
if workflow.count(old) != 1:
    raise SystemExit("unexpected verifier evidence anchor")
workflow_path.write_text(workflow.replace(old, new))
