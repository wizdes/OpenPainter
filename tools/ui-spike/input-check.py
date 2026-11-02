"""W5 real input check. Root runs this as the signed-in user, on the visible desktop only.

Usage: input-check.py <exe> <result folder> <name>

It starts one shell in --measure mode, sends real pointer, wheel, keyboard and replayed pen input
to it, saves two screenshots, then prints the shell's input log. It puts the pointer and the
foreground window back afterwards, and it stops only the process it started.
"""
import ctypes as C
import ctypes.wintypes as W
import json
import pathlib
import subprocess
import sys
import time

def missing_events(events):
    expected = {
        'pointer down': lambda e: e.startswith('pointer:down:'),
        'pointer up': lambda e: e.startswith('pointer:up:'),
        'wheel': lambda e: e.startswith('wheel:'),
        'typed a': lambda e: e == 'text:a',
        'typed é': lambda e: e == 'text:é',
        'Tab': lambda e: e in ('key:9', 'key:Tab'),
        # Avalonia's canvas logs pen:down:0.50. WinUI logs pointer:down:x,y:0.50:Pen.
        'pen pressure 0.50': lambda e: (e.startswith('pen:') or e.endswith(':Pen')) and ':0.50' in e,
        'pen pressure 0.68': lambda e: (e.startswith('pen:') or e.endswith(':Pen')) and ':0.68' in e,
    }
    return [name for name, matches in expected.items() if not any(matches(event) for event in events)]


if sys.argv[1:] == ['--self-test']:
    full = ['pointer:down:1,1:1.00:Mouse', 'pointer:up:1.00:Mouse', 'wheel:120',
            'text:a', 'text:é', 'key:9', 'pen:down:0.50', 'pen:move:0.68']
    assert missing_events(full) == []
    assert missing_events(full[:-2]) == ['pen pressure 0.50', 'pen pressure 0.68']
    assert missing_events(full[:-2] + ['pointer:down:1.0,2.0:0.50:Pen', 'pointer:move:1.0,2.0:0.68:Pen']) == []
    assert missing_events(full[:-2] + ['pointer:down:1.0,2.0:0.50:Mouse', 'pointer:move:1.0,2.0:0.68:Mouse']) == ['pen pressure 0.50', 'pen pressure 0.68']
    assert len(missing_events([])) == 8
    print('INPUT_EVENT_SELF_TEST_PASS 3 cases')
    sys.exit(0)

exe, out_dir, name = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3]
out_dir.mkdir(parents=True, exist_ok=True)
user = C.WinDLL('user32', use_last_error=True)
user.SetProcessDpiAwarenessContext(C.c_void_p(-4))
user.GetForegroundWindow.restype = W.HWND
user.CreateSyntheticPointerDevice.restype = C.c_void_p
user.CreateSyntheticPointerDevice.argtypes = [C.c_int, C.c_uint, C.c_int]


class MOUSE(C.Structure):
    _fields_ = [('dx', C.c_long), ('dy', C.c_long), ('data', C.c_uint), ('flags', C.c_uint), ('time', C.c_uint), ('extra', C.c_size_t)]


class KEY(C.Structure):
    _fields_ = [('vk', C.c_ushort), ('scan', C.c_ushort), ('flags', C.c_uint), ('time', C.c_uint), ('extra', C.c_size_t)]


class UNION(C.Union):
    _fields_ = [('mi', MOUSE), ('ki', KEY), ('pad', C.c_byte * 32)]


class INPUT(C.Structure):
    _fields_ = [('type', C.c_uint), ('u', UNION)]


def send(*items):
    require_front()
    array = (INPUT * len(items))(*items)
    if user.SendInput(len(items), array, C.sizeof(INPUT)) != len(items):
        raise OSError(f'SendInput failed: {C.get_last_error()}')


def mouse(flags, data=0):
    i = INPUT(type=0)
    i.u.mi = MOUSE(0, 0, data, flags, 0, 0)
    return i


def key(vk=0, scan=0, flags=0):
    i = INPUT(type=1)
    i.u.ki = KEY(vk, scan, flags, 0, 0)
    return i


def tap(vk):
    send(key(vk), key(vk, flags=2))
    time.sleep(0.25)


def windows_of(pid):
    found = []
    proto = C.WINFUNCTYPE(C.c_bool, W.HWND, W.LPARAM)

    def visit(hwnd, _):
        owner = W.DWORD()
        user.GetWindowThreadProcessId(hwnd, C.byref(owner))
        if owner.value == pid and user.IsWindowVisible(hwnd):
            r = W.RECT()
            user.GetWindowRect(hwnd, C.byref(r))
            found.append((hwnd, r.left, r.top, r.right, r.bottom))
        return True
    user.EnumWindows(proto(visit), 0)
    return found


target_hwnd = None


def require_front():
    if target_hwnd is None or user.GetForegroundWindow() != target_hwnd:
        raise RuntimeError('NOT_IN_FRONT')


def move_cursor(x, y):
    require_front()
    if not user.SetCursorPos(x, y):
        raise OSError(f'SetCursorPos failed: {C.get_last_error()}')


def shot(rect, path):
    left, top, right, bottom = rect
    subprocess.run(['powershell', '-NoProfile', '-Command', f'''
Add-Type -AssemblyName System.Drawing
$b = New-Object Drawing.Bitmap {right - left},{bottom - top}
$g = [Drawing.Graphics]::FromImage($b)
$g.CopyFromScreen({left},{top},0,0,$b.Size)
$b.Save('{path}'); $g.Dispose(); $b.Dispose()'''], check=True)


def inject_pen(x, y):
    """Replays a pen stroke through Windows' synthetic pen device, which sends pressure with no real pen."""
    class POINTER_INFO(C.Structure):
        _fields_ = [('type', C.c_int), ('id', C.c_uint), ('frame', C.c_uint), ('flags', C.c_uint), ('device', C.c_void_p), ('hwnd', W.HWND),
                    ('pixel', W.POINT), ('himetric', W.POINT), ('pixel_hist', W.POINT), ('himetric_hist', W.POINT), ('time', C.c_uint), ('history', C.c_uint),
                    ('data', C.c_int), ('keys', C.c_uint), ('perf', C.c_ulonglong), ('button', C.c_int)]

    class POINTER_PEN_INFO(C.Structure):
        _fields_ = [('info', POINTER_INFO), ('pen_flags', C.c_uint), ('pen_mask', C.c_uint), ('pressure', C.c_uint), ('rotation', C.c_uint), ('tilt_x', C.c_int), ('tilt_y', C.c_int)]

    class POINTER_TYPE_INFO(C.Structure):
        _fields_ = [('type', C.c_int), ('pad', C.c_int), ('pen', POINTER_PEN_INFO), ('more', C.c_byte * 64)]

    device = user.CreateSyntheticPointerDevice(3, 1, 1)
    if not device:
        raise OSError(f'CreateSyntheticPointerDevice failed: {C.get_last_error()}')
    INRANGE, INCONTACT, DOWN, UPDATE, UP = 0x2, 0x4, 0x10000, 0x20000, 0x40000
    results = []
    for flags, pressure, dx in ((INRANGE | UPDATE, 0, 0), (INRANGE | INCONTACT | DOWN, 512, 0), (INRANGE | INCONTACT | UPDATE, 700, 10), (INRANGE | UP, 0, 10)):
        p = POINTER_TYPE_INFO(type=3)
        p.pen.info.type = 3
        p.pen.info.id = 1
        p.pen.info.flags = flags
        p.pen.info.pixel = W.POINT(x + dx, y)
        p.pen.pen_mask = 1
        p.pen.pressure = pressure
        require_front()
        ok = user.InjectSyntheticPointerInput(C.c_void_p(device), C.byref(p), 1)
        if not ok:
            raise OSError(f'InjectSyntheticPointerInput failed: {C.get_last_error()}')
        results.append(1)
        time.sleep(0.1)
    user.DestroySyntheticPointerDevice(C.c_void_p(device))
    return results


before_fg = user.GetForegroundWindow()
before_pos = W.POINT()
user.GetCursorPos(C.byref(before_pos))
data = out_dir / f'{name}-input.json'
data.unlink(missing_ok=True)
process = subprocess.Popen([str(exe), '--measure', str(data)], cwd=str(exe.parent))
report = {}
try:
    time.sleep(3)
    wins = windows_of(process.pid)
    if not wins:
        raise RuntimeError('NO_WINDOW')
    main = max(wins, key=lambda w: (w[3] - w[1]) * (w[4] - w[2]))
    hwnd, left, top, right, bottom = main
    target_hwnd = hwnd
    report['windows'] = len(wins)
    report['rect'] = [left, top, right, bottom]
    # Windows lets a background program take the front only just after a key press, so tap Alt first.
    user.keybd_event(0x12, 0, 0, 0)
    user.keybd_event(0x12, 0, 2, 0)
    report['foreground'] = bool(user.SetForegroundWindow(hwnd))
    time.sleep(0.4)
    require_front()
    shot((left, top, right, bottom), out_dir / f'{name}-a.png')
    x, y = left + (right - left) * 3 // 5, top + (bottom - top) * 3 // 5
    move_cursor(x, y)
    time.sleep(0.2)
    send(mouse(2))
    time.sleep(0.05)
    move_cursor(x + 12, y + 6)
    time.sleep(0.05)
    send(mouse(4))
    time.sleep(0.2)
    send(mouse(0x800, 120))
    time.sleep(0.2)
    tap(0x41)
    send(key(scan=0xE9, flags=4), key(scan=0xE9, flags=4 | 2))
    time.sleep(0.25)
    tap(0x09)
    tap(0x09)
    report['pen'] = inject_pen(x, y)
    time.sleep(0.5)
    shot((left, top, right, bottom), out_dir / f'{name}-b.png')
    process.wait(timeout=40)
    report['exit'] = process.returncode
except subprocess.TimeoutExpired:
    report['exit'] = 'TIMEOUT'
except (RuntimeError, OSError) as e:
    report['error'] = str(e)
finally:
    if process.poll() is None:
        process.kill()
        process.wait(timeout=5)
    user.SetCursorPos(before_pos.x, before_pos.y)
    if before_fg:
        user.SetForegroundWindow(before_fg)
if data.exists():
    d = json.load(open(data, encoding='utf-8-sig'))
    report['events'] = d.get('InputEvents')
    report['first_ms'] = d.get('FirstFrameMs')
report['missing'] = missing_events(report.get('events') or [])
print(json.dumps(report, indent=0)[:3000])
if report.get('error') or report.get('exit') != 0 or report['missing']:
    sys.exit('INPUT_CHECK_FAILED: ' + str(report.get('error') or ', '.join(report['missing']) or report.get('exit')))
