'''GNOME session manager mock for the Linux L05 real-provider oracle.

Adapted from at-spi2-core 2.52.0 tests/dbusmock/mock-gnome-session.py.
This keeps the accessibility launcher/registry under the same session lifecycle
used by the upstream AT-SPI end-to-end CI rather than a bare dbus-run-session.
'''

# SPDX-License-Identifier: LGPL-3.0-or-later
# Upstream copyright (c) 2022 Federico Mena Quintero.

import dbus
import dbusmock
from dbusmock import MOCK_IFACE

MAIN_IFACE = 'org.gnome.SessionManager'
CLIENT_PRIVATE_IFACE = 'org.gnome.SessionManager.ClientPrivate'


def load(mock, parameters):
    mock.is_running = False
    mock.client_serial_number = 0


@dbus.service.method(MAIN_IFACE, in_signature='', out_signature='b')
def IsSessionRunning(self):
    return self.is_running


@dbus.service.method(MAIN_IFACE, in_signature='ss', out_signature='o')
def RegisterClient(self, app_id, client_startup_id):
    client_name = f"client_{self.client_serial_number}"
    self.client_serial_number += 1
    path = '/org/gnome/SessionManager/MockClientPrivate/' + client_name
    self.AddObject(
        path,
        CLIENT_PRIVATE_IFACE,
        {},
        [('EndSessionResponse', 'bs', '', '')],
    )
    return path


@dbus.service.method(MAIN_IFACE, in_signature='u', out_signature='')
def Logout(self, logout_type):
    assert logout_type == 0
    for path in dbusmock.get_objects():
        if 'MockClientPrivate/' in path:
            obj = dbusmock.get_object(path)
            obj.EmitSignal(CLIENT_PRIVATE_IFACE, 'EndSession', 'u', [0])
            obj.EmitSignal(CLIENT_PRIVATE_IFACE, 'Stop', '', [])


@dbus.service.method(MOCK_IFACE, in_signature='b', out_signature='')
def SetSessionRunning(self, is_running):
    self.is_running = is_running
    self.EmitSignal(MAIN_IFACE, 'SessionRunning', '', ())
