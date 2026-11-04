#!/usr/bin/env python3
"""Stand-in desktop notification server for the E2E bus (R3.5): owns
org.freedesktop.Notifications on the test's private session bus, logs every
Notify / CloseNotification to argv[1], and has a test-only method
Invoke(id, action) that emits ActivationToken + ActionInvoked exactly like
GNOME Shell does when the user clicks the notification or its button.
A Notify reply can be delayed (env NOTIFYD_REPLY_DELAY_MS, or at run time the
test-only method SetReplyDelay(ms)): a slow server, so a client has to replace
a banner whose first reply has not arrived yet."""
import os
import sys
import warnings

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

warnings.filterwarnings("ignore")
XML = """<node><interface name="org.freedesktop.Notifications">
<method name="Notify"><arg type="s" direction="in"/><arg type="u" direction="in"/>
 <arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/>
 <arg type="as" direction="in"/><arg type="a{sv}" direction="in"/><arg type="i" direction="in"/>
 <arg type="u" direction="out"/></method>
<method name="CloseNotification"><arg type="u" direction="in"/></method>
<method name="GetCapabilities"><arg type="as" direction="out"/></method>
<method name="GetServerInformation"><arg type="s" direction="out"/><arg type="s" direction="out"/>
 <arg type="s" direction="out"/><arg type="s" direction="out"/></method>
<method name="SetReplyDelay"><arg type="u" direction="in"/></method>
<method name="Invoke"><arg type="u" direction="in"/><arg type="s" direction="in"/></method>
<signal name="ActionInvoked"><arg type="u"/><arg type="s"/></signal>
<signal name="NotificationClosed"><arg type="u"/><arg type="u"/></signal>
<signal name="ActivationToken"><arg type="u"/><arg type="s"/></signal>
</interface></node>"""
PATH, IFACE = "/org/freedesktop/Notifications", "org.freedesktop.Notifications"
log = open(sys.argv[1], "a", buffering=1)
node = Gio.DBusNodeInfo.new_for_xml(XML)
count = [0]
delay = [int(os.environ.get("NOTIFYD_REPLY_DELAY_MS", "0") or 0)]


def reply(inv, value):
    if delay[0] <= 0:
        inv.return_value(value)
        return
    GLib.timeout_add(delay[0], lambda: inv.return_value(value) and False)


def call(conn, sender, path, iface, method, params, inv):
    if method == "Notify":
        app, replaces, _, summary, body, actions, hints, _ = params.unpack()
        if replaces:  # updates that notification in place (spec), logged apart
            log.write(f"REPLACE {replaces}\t{summary}\t{body!r}\t{actions}\n")
            reply(inv, GLib.Variant("(u)", (replaces,)))
            return
        count[0] += 1
        log.write(f"NOTIFY {count[0]}\t{summary}\t{body!r}\t{actions}\n")
        reply(inv, GLib.Variant("(u)", (count[0],)))
    elif method == "CloseNotification":
        log.write(f"CLOSE {params.unpack()[0]}\n")
        inv.return_value(None)
    elif method == "GetCapabilities":
        inv.return_value(GLib.Variant("(as)", (["actions", "body"],)))
    elif method == "GetServerInformation":
        inv.return_value(GLib.Variant("(ssss)", ("rade-e2e", "rade", "1", "1.2")))
    elif method == "SetReplyDelay":
        delay[0] = params.unpack()[0]
        inv.return_value(None)
    elif method == "Invoke":
        nid, action = params.unpack()
        for sig, args in (("ActivationToken", (nid, "e2e-token")), ("ActionInvoked", (nid, action))):
            conn.emit_signal(None, PATH, IFACE, sig, GLib.Variant("(us)", args))
        log.write(f"INVOKE {nid} {action}\n")
        inv.return_value(None)


def acquired(conn, name):
    conn.register_object(PATH, node.interfaces[0], call, None, None)


Gio.bus_own_name(Gio.BusType.SESSION, IFACE, 0, acquired, None, None)
GLib.MainLoop().run()
