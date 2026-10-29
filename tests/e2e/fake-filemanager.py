#!/usr/bin/env python3
"""Stand-in file manager for the E2E bus (R129-f): owns
org.freedesktop.FileManager1 on the test's private session bus and logs every
ShowItems / ShowFolders / ShowItemProperties call to argv[1] as
`<method> <uri> <uri>…` (one line per call)."""
import sys
import warnings

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

warnings.filterwarnings("ignore")
XML = """<node><interface name="org.freedesktop.FileManager1">
<method name="ShowFolders"><arg type="as" direction="in"/><arg type="s" direction="in"/></method>
<method name="ShowItems"><arg type="as" direction="in"/><arg type="s" direction="in"/></method>
<method name="ShowItemProperties"><arg type="as" direction="in"/><arg type="s" direction="in"/></method>
</interface></node>"""
PATH, IFACE = "/org/freedesktop/FileManager1", "org.freedesktop.FileManager1"
log = open(sys.argv[1], "a", buffering=1)
node = Gio.DBusNodeInfo.new_for_xml(XML)


def call(conn, sender, path, iface, method, params, inv):
    uris, _ = params.unpack()
    log.write(f"{method} {' '.join(uris)}\n")
    inv.return_value(None)


def acquired(conn, name):
    conn.register_object(PATH, node.interfaces[0], call, None, None)


Gio.bus_own_name(Gio.BusType.SESSION, IFACE, 0, acquired, None, None)
GLib.MainLoop().run()
