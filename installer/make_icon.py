"""Writes crates/cf-app/assets/clipframe.ico: the tray icon's corner-bracket design at several
sizes (PNG-compressed ICO entries, anti-aliased by 4x4 supersampling). Standard library only.

Placeholder artwork; rerun after changing the design:  python installer/make_icon.py
"""
import pathlib
import struct
import zlib

SIZES = [16, 20, 24, 32, 40, 48, 64, 256]
COLOR = (0x4F, 0xA3, 0xFF)
# Proportions of the 32-pixel tray icon (see crates/cf-app/src/tray_icon_image.rs).
INSET, STROKE, ARM, DOT_START = 2 / 32, 4 / 32, 12 / 32, 13 / 32
SAMPLES = 4


def lit(u, v):
    """Whether the point (u, v) in 0..1 is inside the design."""
    du, dv = min(u, 1 - u), min(v, 1 - v)
    in_stroke = lambda d: INSET <= d < INSET + STROKE
    in_arm = lambda d: INSET <= d < INSET + ARM
    bracket = (in_stroke(du) and in_arm(dv)) or (in_stroke(dv) and in_arm(du))
    dot = du >= DOT_START and dv >= DOT_START
    return bracket or dot


def render(size):
    rows = []
    for y in range(size):
        row = bytearray([0])  # PNG filter: none
        for x in range(size):
            hits = sum(
                lit((x + (sx + 0.5) / SAMPLES) / size, (y + (sy + 0.5) / SAMPLES) / size)
                for sy in range(SAMPLES)
                for sx in range(SAMPLES)
            )
            row += bytes(COLOR) + bytes([round(255 * hits / SAMPLES**2)])
        rows.append(bytes(row))
    return png(size, b''.join(rows))


def png(size, raw):
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    header = struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0)  # 8-bit RGBA
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', header) + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b'')


def ico(images):
    out = struct.pack('<HHH', 0, 1, len(images))
    offset = 6 + 16 * len(images)
    for size, data in images:
        side = 0 if size >= 256 else size
        out += struct.pack('<BBBBHHII', side, side, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return out + b''.join(data for _, data in images)


if __name__ == '__main__':
    target = pathlib.Path(__file__).resolve().parent.parent / 'crates' / 'cf-app' / 'assets' / 'clipframe.ico'
    target.write_bytes(ico([(size, render(size)) for size in SIZES]))
    print(f'wrote {target} ({target.stat().st_size} bytes)')
