"""KWin/libinput mouse settings. Only a unique supported MCHOSE pointer is eligible."""
import sys, math
from gi.repository import Gio, GLib
C=None
I='org.kde.KWin.InputDevice'
P='/org/kde/KWin/InputDevice'
def call(path, interface, method, args=None):
    return C.call_sync('org.kde.KWin',path,interface,method,args,None,Gio.DBusCallFlags.NONE,2500,None).unpack()[0]
def props(path):return call(path,'org.freedesktop.DBus.Properties','GetAll',GLib.Variant('(s)',(I,)))
def selected():
    found=[]
    for event in call(P,I+'Manager','ListPointers'):
        path=P+'/'+event
        p=props(path)
        if p.get('vendor') in (0x5253,0x3837) and not p.get('touchpad') and p.get('supportsPointerAcceleration'):
            found.append((path,p))
    if len(found)!=1:raise RuntimeError('需要唯一一只受支持的迈从鼠标 / Expected one supported MCHOSE pointer, found '+str(len(found)))
    return found[0]
def setting(p):
    if p.get('pointerAccelerationProfileFlat'):flat=True
    elif p.get('pointerAccelerationProfileAdaptive'):flat=False
    else:raise RuntimeError('Unsupported acceleration profile / 不支持的加速度模式')
    return p['pointerAcceleration'],flat
try:
    C=Gio.bus_get_sync(Gio.BusType.SESSION,None)
    path,p=selected()
    speed,flat=setting(p)
    if len(sys.argv)>1:
        speed=float(sys.argv[1]);flat={'true':True,'false':False}[sys.argv[2]]
        if not math.isfinite(speed) or not -1<=speed<=1:raise ValueError('speed must be -1..1')
        prop='pointerAccelerationProfile'+('Flat' if flat else 'Adaptive')
        if not p.get('supports'+prop[0].upper()+prop[1:]):raise RuntimeError('Unsupported acceleration profile')
        def setprop(key,value):
            C.call_sync('org.kde.KWin',path,'org.freedesktop.DBus.Properties','Set',GLib.Variant('(ssv)',(I,key,value)),None,Gio.DBusCallFlags.NONE,2500,None)
        original=setting(p)
        try:
            setprop(prop,GLib.Variant('b',True))
            setprop('pointerAcceleration',GLib.Variant('d',speed))
            actual=setting(props(path))
            if abs(actual[0]-speed)>1e-7 or actual[1]!=flat:raise RuntimeError('System settings readback failed')
        except Exception:
            setprop('pointerAccelerationProfile'+('Flat' if original[1] else 'Adaptive'),GLib.Variant('b',True))
            setprop('pointerAcceleration',GLib.Variant('d',original[0]))
            raise
    print(str(speed)+'\t'+str(flat).lower()+'\t'+p['name'].replace('\t',' ').replace('\n',' '))
except Exception as e:
    print('KDE 系统鼠标设置 / KDE mouse settings: '+str(e),file=sys.stderr);sys.exit(1)
