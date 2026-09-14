#!/usr/bin/env python3
import json
import sys

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Atk", "1.0")
from gi.repository import GLib, Gtk

ACCESSIBLE_NAME = "LocalView L01 Defunct Button"

press_count = 0
button = None
old_accessible = None


def emit(payload):
    sys.stdout.write(json.dumps(payload, sort_keys=True) + "\n")
    sys.stdout.flush()


def on_clicked(_button):
    global press_count
    press_count += 1


def on_stdin(source, condition):
    global button
    if condition & GLib.IO_HUP:
        Gtk.main_quit()
        return False

    line = source.readline()
    if not line:
        Gtk.main_quit()
        return False

    command = line.strip()
    if command == "destroy":
        # Keep the AtkObject itself alive/exported, but sever its GTK backing
        # association through GTK's public accessibility API. GTK3's
        # GtkWidgetAccessible::ref_state_set then derives ATK_STATE_DEFUNCT
        # from the missing backing widget. This deliberately does not inject
        # an ATK state or send provider state over the control channel.
        if old_accessible is not None:
            old_accessible.set_widget(None)
        emit({"event": "destroyed"})
    elif command == "status":
        emit({"event": "status", "press_count": press_count})
    elif command == "quit":
        emit({"event": "quitting"})
        Gtk.main_quit()
        return False
    else:
        emit({"event": "error", "command": command})
    return True


def main():
    global button, old_accessible

    window = Gtk.Window(title="LocalView L01 DEFUNCT Seed")
    window.set_default_size(360, 120)
    window.connect("destroy", lambda _window: None)

    button = Gtk.Button(label=ACCESSIBLE_NAME)
    button.connect("clicked", on_clicked)
    old_accessible = button.get_accessible()
    old_accessible.set_name(ACCESSIBLE_NAME)

    window.add(button)
    window.show_all()

    GLib.io_add_watch(sys.stdin, GLib.IO_IN | GLib.IO_HUP, on_stdin)
    emit({"event": "ready"})
    Gtk.main()


if __name__ == "__main__":
    main()
