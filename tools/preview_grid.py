#!/usr/bin/env python3
"""Build assets/scene-data/images/preview-grid.png, the floor under material previews.

A checker of labelled tiles (A1 to H8) with a fine grid: the labels and lines show how a
material bends, blurs or mirrors what is around it.
"""
import struct
import zlib
from pathlib import Path

TILES = 8
TILE = 128
SIZE = TILES * TILE
MINOR = TILE // 4
SCALE = 4  # Pixels per font dot.
LIGHT, DARK = (226, 226, 222), (204, 204, 200)
MINOR_LINE, MAJOR_LINE, INK = (186, 186, 182), (96, 96, 94), (64, 64, 62)

# A 5x7 dot font, drawn for this file.
FONT = {
    "A": ["01110", "10001", "10001", "11111", "10001", "10001", "10001"],
    "B": ["11110", "10001", "10001", "11110", "10001", "10001", "11110"],
    "C": ["01111", "10000", "10000", "10000", "10000", "10000", "01111"],
    "D": ["11110", "10001", "10001", "10001", "10001", "10001", "11110"],
    "E": ["11111", "10000", "10000", "11110", "10000", "10000", "11111"],
    "F": ["11111", "10000", "10000", "11110", "10000", "10000", "10000"],
    "G": ["01111", "10000", "10000", "10011", "10001", "10001", "01111"],
    "H": ["10001", "10001", "10001", "11111", "10001", "10001", "10001"],
    "1": ["00100", "01100", "00100", "00100", "00100", "00100", "01110"],
    "2": ["01110", "10001", "00001", "00010", "00100", "01000", "11111"],
    "3": ["11110", "00001", "00001", "01110", "00001", "00001", "11110"],
    "4": ["00010", "00110", "01010", "10010", "11111", "00010", "00010"],
    "5": ["11111", "10000", "11110", "00001", "00001", "10001", "01110"],
    "6": ["00110", "01000", "10000", "11110", "10001", "10001", "01110"],
    "7": ["11111", "00001", "00010", "00100", "01000", "01000", "01000"],
    "8": ["01110", "10001", "10001", "01110", "10001", "10001", "01110"],
}


def ink_mask():
    """Pixels covered by each tile's label, from the dot font."""
    mask = set()
    for row in range(TILES):
        for column in range(TILES):
            label = "ABCDEFGH"[column] + str(row + 1)
            left, top = column * TILE + 12, row * TILE + 12
            for index, char in enumerate(label):
                for y, line in enumerate(FONT[char]):
                    for x, dot in enumerate(line):
                        if dot == "1":
                            gx = left + index * 6 * SCALE + x * SCALE
                            gy = top + y * SCALE
                            mask.update((gx + i, gy + j) for i in range(SCALE) for j in range(SCALE))
    return mask


def build():
    mask = ink_mask()
    rows = bytearray()
    # The floor reads the texture bottom row first, so the rows are written bottom up and the
    # labels read the right way on the floor.
    for y in reversed(range(SIZE)):
        rows.append(0)
        for x in range(SIZE):
            if x % TILE < 3 or y % TILE < 3:
                color = MAJOR_LINE
            elif (x, y) in mask:
                color = INK
            elif x % MINOR == 0 or y % MINOR == 0:
                color = MINOR_LINE
            else:
                color = LIGHT if (x // TILE + y // TILE) % 2 == 0 else DARK
            rows.extend(color)
    return bytes(rows)


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)


png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 2, 0, 0, 0))
       + chunk(b"IDAT", zlib.compress(build(), 9)) + chunk(b"IEND", b""))
out = Path(__file__).resolve().parent.parent / "assets" / "scene-data" / "images" / "preview-grid.png"
out.write_bytes(png)
print(f"{out}: {len(png)} bytes")
