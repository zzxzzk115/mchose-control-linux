"""Native StatusNotifierItem + DBusMenu; stdout is a private parent event channel."""
import sys, os
from gi.repository import Gio, GLib
c=Gio.bus_get_sync(Gio.BusType.SESSION,None)
zh=sys.argv[1]=='zh'
loop=GLib.MainLoop()
def emit(s):print(s,flush=True)
sni='org.kde.StatusNotifierItem';menu='com.canonical.dbusmenu'
xml='''<node><interface name="org.kde.StatusNotifierItem">
<method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="SecondaryActivate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="ContextMenu"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="Scroll"><arg type="i" direction="in"/><arg type="s" direction="in"/></method>
<property name="Category" type="s" access="read"/><property name="Id" type="s" access="read"/>
<property name="Title" type="s" access="read"/><property name="Status" type="s" access="read"/>
<property name="IconName" type="s" access="read"/><property name="ItemIsMenu" type="b" access="read"/>
<property name="Menu" type="o" access="read"/><property name="ToolTip" type="(sa(iiay)ss)" access="read"/>
</interface><interface name="com.canonical.dbusmenu">
<method name="GetLayout"><arg type="i" direction="in"/><arg type="i" direction="in"/><arg type="as" direction="in"/><arg type="u" direction="out"/><arg type="(ia{sv}av)" direction="out"/></method>
<method name="GetGroupProperties"><arg type="ai" direction="in"/><arg type="as" direction="in"/><arg type="a(ia{sv})" direction="out"/></method>
<method name="Event"><arg type="i" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="in"/><arg type="u" direction="in"/></method>
<method name="AboutToShow"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
<property name="Version" type="u" access="read"/><property name="TextDirection" type="s" access="read"/><property name="Status" type="s" access="read"/>
</interface></node>'''
def props(i):
    labels={1:'打开设置' if zh else 'Open settings',2:'启用应用自动切换' if zh else 'Enable automatic presets',3:'暂停并切回 desktop' if zh else 'Pause and apply desktop',4:'退出并切回 desktop' if zh else 'Quit and apply desktop'}
    if i==0:return {'children-display':GLib.Variant('s','submenu')}
    return {'label':GLib.Variant('s',labels[i]),'enabled':GLib.Variant('b',True),'visible':GLib.Variant('b',True)}
def action(i):
    if i in (1,2,3,4):emit({1:'show',2:'start',3:'stop',4:'quit'}[i])
def method(conn,sender,path,iface,name,args,inv):
    try:
        a=args.unpack()
        if iface==sni:
            if name in ('Activate','SecondaryActivate','ContextMenu'):emit('show')
            inv.return_value(None)
        elif name=='GetLayout':
            i=a[0];children=[GLib.Variant('(ia{sv}av)',(j,props(j),[])) for j in (1,2,3,4)] if i==0 else []
            inv.return_value(GLib.Variant('(u(ia{sv}av))',(1,(i,props(i),children))))
        elif name=='GetGroupProperties':inv.return_value(GLib.Variant('(a(ia{sv}))',([(i,props(i)) for i in (a[0] or [0,1,2,3,4])],)))
        elif name=='Event':
            if a[1]=='clicked':action(a[0])
            inv.return_value(None)
        elif name=='AboutToShow':inv.return_value(GLib.Variant('(b)',(False,)))
    except Exception as e:inv.return_dbus_error('org.mchose.Error',str(e))
def prop(conn,sender,path,iface,name):
    if iface==menu:return {'Version':GLib.Variant('u',3),'TextDirection':GLib.Variant('s','ltr'),'Status':GLib.Variant('s','normal')}[name]
    values={'Category':('s','Hardware'),'Id':('s','mchose'),'Title':('s','迈从鼠标' if zh else 'MCHOSE Mouse'),'Status':('s','Active'),'IconName':('s','mchose'),'ItemIsMenu':('b',False),'Menu':('o','/Menu'),'ToolTip':('(sa(iiay)ss)',('mchose',[],'MCHOSE Mouse','点击打开设置' if zh else 'Click to open settings'))}
    return GLib.Variant(*values[name])
info=Gio.DBusNodeInfo.new_for_xml(xml)
c.register_object('/StatusNotifierItem',info.interfaces[0],method,prop,None)
c.register_object('/Menu',info.interfaces[1],method,prop,None)
def register():
    try:
        c.call_sync('org.kde.StatusNotifierWatcher','/StatusNotifierWatcher','org.kde.StatusNotifierWatcher','RegisterStatusNotifierItem',GLib.Variant('(s)',(c.get_unique_name(),)),None,Gio.DBusCallFlags.NONE,2500,None)
        emit('ready')
    except Exception:emit('unavailable')
def owner_changed(conn,sender,path,iface,name,args):
    if args.unpack()[0]=='org.kde.StatusNotifierWatcher':register()
c.signal_subscribe('org.freedesktop.DBus','org.freedesktop.DBus','NameOwnerChanged','/org/freedesktop/DBus','org.kde.StatusNotifierWatcher',Gio.DBusSignalFlags.NONE,owner_changed)
parent=os.getppid()
def alive():
    if os.getppid()!=parent:loop.quit();return False
    return True
GLib.timeout_add_seconds(2,alive)
register();loop.run()
