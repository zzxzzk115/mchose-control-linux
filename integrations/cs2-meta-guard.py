#!/usr/bin/python3
"""Suspend Meta desktop shortcuts while native CS2 has focus; preserve Alt+Tab."""
import json, os, pathlib, signal, sys, fcntl
from gi.repository import Gio, GLib
BUS = 'io.github.mchose.CS2MetaGuard'
PATH = '/io/github/mchose/CS2MetaGuard'
SCRIPT = 'cs2-meta-guard'
STATE = pathlib.Path(os.environ.get('XDG_STATE_HOME', pathlib.Path.home()/'.local/state'))/'cs2-meta-guard'
STATE.mkdir(parents=True, exist_ok=True)
JOURNAL = STATE/'pending-restore.json'
lock = open(STATE/'lock', 'w')
fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
conn = Gio.bus_get_sync(Gio.BusType.SESSION, None)
def call(dest, path, interface, method, args=None):
    result = conn.call_sync(dest, path, interface, method, args, None, Gio.DBusCallFlags.NONE, 2500, None).unpack()
    return result[0] if len(result)==1 else result

def accel(method, args=None):
    return call('org.kde.kglobalaccel','/kglobalaccel','org.kde.KGlobalAccel',method,args)
def keys(action):
    return [list(s[0]) for s in accel('shortcutKeys', GLib.Variant('(as)', (action,)))]
def meta(sequence):
    return any(k == 0x01000022 or k & 0x10000000 for k in sequence)
def normalize(sequences):
    return sorted(tuple(s) for s in sequences if any(s))
def set_keys(action, sequences):
    result=accel('setShortcutKeys',GLib.Variant('(asa(ai)u)',(action,[(s,) for s in sequences],4)))
    actual=[list(s[0]) for s in result]
    if normalize(actual)!=normalize(sequences):raise RuntimeError('KDE refused shortcut update: '+str(action[:2]))
def save(entries):
    temp=JOURNAL.with_suffix('.tmp');temp.write_text(json.dumps(entries));temp.chmod(0o600);temp.replace(JOURNAL)
def restore():
    if not JOURNAL.exists():return
    entries=json.loads(JOURNAL.read_text());remaining=[]
    for entry in entries:
        try:
            current=keys(entry['action'])
            # Keep any non-Meta edits made while gaming; otherwise restore exactly.
            original=entry['before']
            target=original if normalize(current)==normalize(entry['during']) else current+[s for s in original if meta(s) and s not in current]
            set_keys(entry['action'],target)
        except Exception as e:
            print('restore failed:',e,flush=True);remaining.append(entry)
    if remaining:save(remaining);raise RuntimeError('Some Meta shortcuts could not be restored')
    JOURNAL.unlink(missing_ok=True)
    print('Meta shortcuts restored; Alt+Tab unchanged',flush=True)
def block():
    if JOURNAL.exists():return
    entries=[]
    for component in accel('allMainComponents'):
        for action in accel('allActionsForComponent',GLib.Variant('(as)',(component,))):
            before=keys(action)
            if any(meta(s) for s in before):entries.append({'action':action,'before':before,'during':[s for s in before if not meta(s)]})
    save(entries) # Recovery is durable before the first change.
    try:
        for entry in entries:set_keys(entry['action'],entry['during'])
    except Exception:
        restore();raise
    print('CS2 focused: Meta desktop shortcuts blocked; Alt+Tab unchanged',flush=True)
def kwin(method,args=None):
    return call('org.kde.KWin','/Scripting','org.kde.kwin.Scripting',method,args)

def main():
    restore()
    if '--restore' in sys.argv:return
    if '--self-test' in sys.argv:
        assert meta([0x01000022,0,0,0]) and meta([0x10000041,0,0,0])
        assert not meta([0x09000001,0,0,0]) # Alt+Tab
        action=['kwin','Walk Through Windows','KWin','Walk Through Windows']
        before=keys(action)
        try:
            block()
            assert [s for s in before if not meta(s)]==keys(action) or normalize([s for s in before if not meta(s)])==normalize(keys(action))
            for entry in json.loads(JOURNAL.read_text()):assert not any(meta(s) for s in keys(entry['action']))
        finally:restore()
        assert normalize(before)==normalize(keys(action))
        print('PASS: Meta-only filtering, Alt+Tab preservation, readback and restoration',flush=True)
        return
    owner=call('org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','GetNameOwner',GLib.Variant('(s)',('org.kde.KWin',)))
    result=call('org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','RequestName',GLib.Variant('(su)',(BUS,4)))
    if result not in (1,4):raise RuntimeError('Guard is already running')
    loop=GLib.MainLoop()
    focused=False
    def update(connection,sender,path,interface,method,params,invocation):
        nonlocal focused
        if sender!=owner:
            invocation.return_dbus_error(BUS+'.Denied','Only KWin may report focus');return
        app,klass,pid=params.unpack()
        try:exe=pathlib.Path('/proc',str(pid),'exe').readlink().name if pid else ''
        except OSError:exe=''
        wanted=app in ('steam_app_730','cs2') or klass.lower() in ('steam_app_730','cs2') or exe=='cs2'
        try:
            if wanted!=focused:
                if wanted:block()
                else:restore()
                focused=wanted
            invocation.return_value(None)
        except Exception as e:
            print('focus update failed:',e,flush=True);invocation.return_dbus_error(BUS+'.Error',str(e));loop.quit()
    xml=f'<node><interface name="{BUS}"><method name="Update"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/></method></interface></node>'
    info=Gio.DBusNodeInfo.new_for_xml(xml)
    registration=conn.register_object(PATH,info.interfaces[0],update,None,None)
    js=pathlib.Path(os.environ['XDG_RUNTIME_DIR'])/'cs2-meta-guard.js'
    js.write_text(f'''function cs2MetaGuardReport(w) {{
      callDBus("{BUS}", "{PATH}", "{BUS}", "Update", w ? String(w.desktopFileName || "") : "", w ? String(w.resourceClass || "") : "", w ? String(w.pid || 0) : "0");
    }}
    workspace.windowActivated.connect(cs2MetaGuardReport);
    cs2MetaGuardReport(workspace.activeWindow);
    ''')
    for sig in (signal.SIGTERM,signal.SIGINT):GLib.unix_signal_add(GLib.PRIORITY_DEFAULT,sig,lambda:(loop.quit(),False)[1])
    def owner_changed(connection,sender,path,interface,name,params):
        bus,old,new=params.unpack()
        if bus in ('org.kde.KWin','org.kde.kglobalaccel') and old and old!=new:loop.quit()
    conn.signal_subscribe('org.freedesktop.DBus','org.freedesktop.DBus','NameOwnerChanged','/org/freedesktop/DBus',None,Gio.DBusSignalFlags.NONE,owner_changed)
    try:
        kwin('unloadScript',GLib.Variant('(s)',(SCRIPT,)))
        sid=kwin('loadScript',GLib.Variant('(ss)',(str(js),SCRIPT)))
        if sid<0:raise RuntimeError('KWin refused the focus script')
        call('org.kde.KWin','/Scripting/Script'+str(sid),'org.kde.kwin.Script','run')
        print('Ready: watching CS2 foreground windows',flush=True)
        loop.run()
    finally:
        try:kwin('unloadScript',GLib.Variant('(s)',(SCRIPT,)))
        finally:restore();conn.unregister_object(registration)
if __name__=='__main__':main()
