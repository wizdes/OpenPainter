# Writes the checked-in M38 tablet log: two pen strokes over a 512 x 384 image
# with a hover between them. Pressure is on the 16-bit tablet grid (k / 65535)
# and tilt on the 15-bit grid (k / 32767), so a replay through CGEvent tablet
# fields gives back the same numbers. Positions are multiples of 1/8 px.
import json, math, sys
samples = []
t = 0.0
def q(v, n):  # nearest grid value k / n
    return round(v * n) / n
def add(x, y, p, tx, ty, b):
    global t
    samples.append({"t": round(t, 6), "x": round(x * 8) / 8, "y": round(y * 8) / 8,
                    "pressure": q(max(0, min(1, p)), 65535), "tilt": [q(tx, 32767), q(ty, 32767)], "buttons": b})
    t += 1 / 240
# Stroke 1: a wave from left to right, pressure rising and falling.
n = 140
for i in range(n):
    u = i / (n - 1)
    add(40 + 420 * u, 190 + 110 * math.sin(u * 3 * math.pi), 0.1 + 0.9 * math.sin(u * math.pi), 0.6 * u - 0.3, -0.4 + 0.5 * u, 1)
add(460, 190, 0.0, 0.3, 0.1, 0)  # release
for i in range(10):  # hover back
    add(460 - 30 * i, 100, 0.0, 0.0, 0.0, 0)
# Stroke 2: a loop with a light touch at the start and a hard press at the end.
m = 90
for i in range(m):
    a = i / (m - 1) * 2 * math.pi
    add(250 + 80 * math.cos(a), 140 + 60 * math.sin(a), 0.05 + 0.95 * i / (m - 1), -0.5, 0.5 * math.cos(a), 1)
add(330, 140, 0.0, 0.0, 0.0, 0)
log = {"format": "openpainter-tablet-log", "version": 1, "space": "document-pixels", "samples": samples}
json.dump(log, sys.stdout, indent=None, separators=(",", ":"))
