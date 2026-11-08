"""Generate the project's synthetic latitude-longitude studio HDR (no external assets)."""
import math
from pathlib import Path

width, height = 512, 256
target = Path(__file__).resolve().parents[1] / "assets/environments/studio.hdr"
target.parent.mkdir(parents=True, exist_ok=True)
with target.open("wb") as out:
    out.write(f"#?RADIANCE\n# Procedural FAR studio environment\nFORMAT=32-bit_rle_rgbe\n\n-Y {height} +X {width}\n".encode())
    for y in range(height):
        latitude = math.pi * (y + 0.5) / height
        for x in range(width):
            longitude = 2 * math.pi * (x + 0.5) / width
            upper = max(0, math.cos(latitude))
            rgb = [0.12 + 0.18 * upper, 0.14 + 0.22 * upper, 0.18 + 0.3 * upper]
            # Two broad studio panels with different color temperatures.
            for center, span, top, bottom, emission in [
                (3.9, 0.36, 0.5, 1.35, (9, 7.5, 5.5)),
                (0.6, 0.23, 0.7, 1.5, (3.5, 5.5, 9)),
            ]:
                angle = abs((longitude - center + math.pi) % (2 * math.pi) - math.pi)
                if angle < span and top < latitude < bottom:
                    rgb = [a + b for a, b in zip(rgb, emission)]
            ceiling = 1.5 * math.exp(-((latitude / 0.4) ** 2))
            rgb = [c + ceiling for c in rgb]
            peak = max(rgb)
            mantissa, exponent = math.frexp(peak)
            scale = mantissa * 256 / peak
            out.write(bytes([min(255, int(c * scale)) for c in rgb] + [exponent + 128]))
print(target)
