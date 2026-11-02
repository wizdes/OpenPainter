"""W5 pan check. Root runs this as the signed-in user, on the visible desktop only.

Usage: pan-check.py <exe> <result json>

It starts one shell with SPIKE_PAN=1, sweeps the real pointer over the canvas from 3 s to 9 s,
then prints the engine's frame rate and gaps in that time. Each pointer move changes the view, and
nothing else asks for a frame. Before each move it checks that the shell's window is still in
front, and it stops if not. It puts the pointer and the foreground window back afterwards.
"""
import ctypes as C
import ctypes.wintypes as W
import json
import os
import subprocess
import sys
import time

exe, out = sys.argv[1], sys.argv[2]
user = C.WinDLL('user32', use_last_error=True)
user.SetProcessDpiAwarenessContext(C.c_void_p(-4))
user.GetForegroundWindow.restype = W.HWND


class MOUSE(C.Structure):
    _fields_ = [('dx', C.c_long), ('dy', C.c_long), ('data', C.c_uint), ('flags', C.c_uint), ('time', C.c_uint), ('extra', C.c_size_t)]


class INPUT(C.Structure):
    _fields_ = [('type', C.c_uint), ('mi', MOUSE)]


def largest_window(pid):
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
    return max(found, key=lambda w: (w[3] - w[1]) * (w[4] - w[2])) if found else None


before_fg = user.GetForegroundWindow()
before_pos = W.POINT()
user.GetCursorPos(C.byref(before_pos))
if os.path.exists(out):
    os.remove(out)
env = dict(os.environ, SPIKE_PAN='1')
start = time.time()
process = subprocess.Popen([exe, '--measure', out], cwd=os.path.dirname(exe), env=env)
moves = 0
try:
    time.sleep(2.5)
    win = largest_window(process.pid)
    if win is None:
        raise SystemExit('NO_WINDOW')
    hwnd, left, top, right, bottom = win
    # Windows lets a background program take the front only just after a key press, so tap Alt first.
    user.keybd_event(0x12, 0, 0, 0)
    user.keybd_event(0x12, 0, 2, 0)
    if not user.SetForegroundWindow(hwnd):
        raise SystemExit('SET_FOREGROUND_FAILED')
    time.sleep(0.3)
    if user.GetForegroundWindow() != hwnd:
        raise SystemExit('NOT_IN_FRONT')
    x0, y = left + 200, (top + bottom) // 2
    if user.GetForegroundWindow() != hwnd:
        raise SystemExit('NOT_IN_FRONT')
    if not user.SetCursorPos(x0, y):
        raise SystemExit(f'SET_CURSOR_FAILED: {C.get_last_error()}')
    step = 4
    while time.time() - start < 9.2:
        if user.GetForegroundWindow() != hwnd:
            raise SystemExit('LOST_FRONT')
        i = INPUT(type=0)
        i.mi = MOUSE(step, 0, 0, 1, 0, 0)
        if user.SendInput(1, C.byref(i), C.sizeof(INPUT)) != 1:
            raise SystemExit(f'SENDINPUT_FAILED: {C.get_last_error()}')
        moves += 1
        if moves % 100 == 0:
            step = -step
        time.sleep(0.004)
    process.wait(timeout=40)
finally:
    if process.poll() is None:
        process.kill()
        process.wait(timeout=5)
    user.SetCursorPos(before_pos.x, before_pos.y)
    if before_fg:
        user.SetForegroundWindow(before_fg)
d = json.load(open(out, encoding='utf-8-sig'))
if process.returncode != 0:
    raise SystemExit(f'PAN_MEASURE_EXIT_{process.returncode}')
t = [x for x in d['FrameTimesMs'] if 3500 <= x <= 9000]
if len(t) < 2:
    raise SystemExit('PAN_MEASURE_FRAMES_MISSING')
g = sorted(b - a for a, b in zip(t, t[1:]))
print(json.dumps({'stack': d['Stack'], 'pointer_moves': moves, 'engine_fps': round(len(t) / 5.5, 1),
                  'gap_median_ms': round(g[len(g) // 2], 1) if g else None, 'gap_p99_ms': round(g[int(len(g) * .99)], 1) if g else None,
                  'gap_max_ms': round(g[-1], 1) if g else None, 'exit': process.returncode}))
