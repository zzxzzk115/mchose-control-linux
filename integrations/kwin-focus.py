#!/usr/bin/env python3
"""Read-only KWin focus bridge. No window manipulation or hardware access."""
import sys, signal, pathlib, urllib.parse, warnings
warnings.filterwarnings("ignore", category=DeprecationWarning)
import gi
gi.require_version('Gio', '2.0')
from gi.repository import Gio, GLib
BUS='io.github.mchose.Focus'
PATH='/io/github/mchose/Focus'
SCRIPT='mchose-focus'
conn=Gio.bus_get_sync(Gio.BusType.SESSION, None)
def call(dest,path,interface,method,params=None):
    return conn.call_sync(dest,path,interface,method,params,None,Gio.DBusCallFlags.NONE,3000,None)
def kwin(method,params=None):return call('org.kde.KWin','/Scripting','org.kde.kwin.Scripting',method,params)
owner=call('org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','GetNameOwner',GLib.Variant('(s)',('org.kde.KWin',))).unpack()[0]
result=call('org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','RequestName',GLib.Variant('(su)',(BUS,4))).unpack()[0]
if result not in (1,4):raise RuntimeError('MCHOSE focus bridge is already running')
xml='''<node><interface name="io.github.mchose.Focus"><method name="Update"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/></method></interface></node>'''
def update(connection,sender,path,interface,method,params,invocation):
    if sender!=owner:
        invocation.return_dbus_error(BUS+'.Denied','Only KWin may update focus');return
    app,klass,title,pid=params.unpack()
    fields=[app,klass,title,str(pid)]
    print('WINDOW\t'+'\t'.join(urllib.parse.quote(str(v)[:2048],safe='') for v in fields),flush=True)
    invocation.return_value(None)
info=Gio.DBusNodeInfo.new_for_xml(xml)
registration=conn.register_object(PATH,info.interfaces[0],update,None,None)
js=pathlib.Path(sys.argv[1])
js.write_text('''function report(w) {
  callDBus("io.github.mchose.Focus", "/io/github/mchose/Focus", "io.github.mchose.Focus", "Update",
    w ? String(w.desktopFileName || "") : "", w ? String(w.resourceClass || "") : "",
    w ? String(w.caption || "") : "", w ? String(w.pid || 0) : "0");
}
workspace.windowActivated.connect(report);
workspace.windowAdded.connect(function(w) {
  w.captionChanged.connect(function() { if (workspace.activeWindow === w) report(w); });
});
workspace.windowList().forEach(function(w) {
  w.captionChanged.connect(function() { if (workspace.activeWindow === w) report(w); });
});
report(workspace.activeWindow);
''')
loop=GLib.MainLoop()
for sig in (signal.SIGTERM,signal.SIGINT):GLib.unix_signal_add(GLib.PRIORITY_DEFAULT,sig,lambda:(loop.quit(),False)[1])
conn.connect('closed',lambda *args:loop.quit())
# Stop if KWin disappears, rather than leaving stale focus data active.
def owner_changed(connection,sender,path,interface,signal_name,params):
    name,old,new=params.unpack()
    if name=='org.kde.KWin' and new!=owner:loop.quit()
conn.signal_subscribe('org.freedesktop.DBus','org.freedesktop.DBus','NameOwnerChanged','/org/freedesktop/DBus','org.kde.KWin',Gio.DBusSignalFlags.NONE,owner_changed)
try:
    kwin('unloadScript',GLib.Variant('(s)',(SCRIPT,)))
    script_id=kwin('loadScript',GLib.Variant('(ss)',(str(js),SCRIPT))).unpack()[0]
    if script_id<0:raise RuntimeError('KWin refused the focus script')
    # Start only our script, not other installed scripts.
    call('org.kde.KWin','/Scripting/Script'+str(script_id),'org.kde.kwin.Script','run')
    print('READY',flush=True)
    loop.run()
finally:
    try:kwin('unloadScript',GLib.Variant('(s)',(SCRIPT,)))
    except Exception:pass
    conn.unregister_object(registration)
