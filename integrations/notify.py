"""Send a passive desktop notification without focus changes."""
import sys,html
from gi.repository import Gio,GLib
try:
 c=Gio.bus_get_sync(Gio.BusType.SESSION,None)
 result=c.call_sync('org.freedesktop.Notifications','/org/freedesktop/Notifications','org.freedesktop.Notifications','Notify',GLib.Variant('(susssasa{sv}i)',('MCHOSE Control',0,'mchose',sys.argv[1],html.escape(sys.argv[2]),[],{'desktop-entry':GLib.Variant('s','mchose'),'urgency':GLib.Variant('y',1)},5000)),None,Gio.DBusCallFlags.NONE,2000,None)
 print(result.unpack()[0])
except Exception as e:
 print(str(e),file=sys.stderr);sys.exit(1)
