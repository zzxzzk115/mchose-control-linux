"""KDE Wayland window lifecycle: hide only our PID from taskbar/switcher."""
import sys,os,pathlib
from gi.repository import Gio,GLib
pid=int(sys.argv[1]);hide=sys.argv[2]=='hide'
c=Gio.bus_get_sync(Gio.BusType.SESSION,None)
def call(path,method,args=None):
 return c.call_sync('org.kde.KWin',path,'org.kde.kwin.Scripting' if path=='/Scripting' else 'org.kde.kwin.Script',method,args,None,Gio.DBusCallFlags.NONE,2500,None).unpack()
folder=pathlib.Path(os.environ['XDG_RUNTIME_DIR'])/'mchose';folder.mkdir(mode=0o700,exist_ok=True)
script=folder/f'window-{os.getpid()}.js';name=f'mchose-window-{os.getpid()}'
script.write_text('workspace.windowList().forEach(function(w){if(w.pid=='+str(pid)+'){w.skipTaskbar='+str(hide).lower()+';w.skipSwitcher='+str(hide).lower()+';w.minimized='+str(hide).lower()+';'+('' if hide else 'workspace.activeWindow=w;')+'}});')
try:
 ident=call('/Scripting','loadScript',GLib.Variant('(ss)',(str(script),name)))[0]
 if ident<0:raise RuntimeError('KWin rejected window lifecycle script')
 call('/Scripting/Script'+str(ident),'run')
finally:
 try:call('/Scripting','unloadScript',GLib.Variant('(s)',(name,)))
 finally:script.unlink(missing_ok=True)
