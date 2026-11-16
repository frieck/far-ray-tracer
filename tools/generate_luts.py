"""Generate the bundled color looks, as .cube LUTs in assets/luts (no external assets).

Each look maps the screen colors of a finished picture, sRGB values from 0 to 1, to new ones,
as a colorist's grade does. 17 points per axis is plenty for these smooth curves, which far-core
reads between with tetrahedral interpolation.
"""
from pathlib import Path

SIZE = 17
folder = Path(__file__).resolve().parents[1] / "assets/luts"


def luma(c):
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]


def mix(a, b, t):
    return a + (b - a) * t


def s_curve(v, amount):
    """More contrast around the middle, ends kept: the smoothstep blended in by `amount`."""
    v = min(1.0, max(0.0, v))
    return mix(v, v * v * (3 - 2 * v), amount)


def saturate(c, amount):
    y = luma(c)
    return [y + (v - y) * amount for v in c]


def levels(c, black, white):
    """Black lifted to `black` and white lowered to `white`, as faded prints show."""
    return [black + v * (white - black) for v in c]


def warm_film(c):
    # A color negative printed warm: soft contrast, lifted blacks, warm mid-tones and highlights.
    c = [s_curve(v, 0.25) for v in c]
    y = luma(c)
    warm = 4 * y * (1 - y)
    c = [c[0] + 0.035 * warm + 0.02 * y, c[1] + 0.01 * warm, c[2] - 0.045 * warm - 0.02 * y]
    c = saturate(c, 0.92)
    return levels(c, 0.025, 0.985)


def teal_orange(c):
    # The cinema look: shadows toward teal, light and skin toward orange, more contrast.
    c = [s_curve(v, 0.35) for v in c]
    y = luma(c)
    shadow, light = (1 - y) ** 2, y ** 2
    c = [c[0] - 0.06 * shadow + 0.06 * light, c[1] + 0.015 * shadow + 0.015 * light, c[2] + 0.07 * shadow - 0.07 * light]
    return saturate(c, 1.12)


def bleach_bypass(c):
    # Silver left in the print: little color, deep blacks, hard contrast.
    y = s_curve(luma(c), 0.6)
    c = saturate(c, 0.45)
    # The silver image laid over the color one, as an overlay.
    over = [2 * v * y if y < 0.5 else 1 - 2 * (1 - v) * (1 - y) for v in c]
    return [mix(v, o, 0.6) for v, o in zip(c, over)]


def black_and_white(c):
    # Panchromatic film: a little more red and less blue than the eye, and a gentle S-curve.
    y = 0.30 * c[0] + 0.60 * c[1] + 0.10 * c[2]
    y = s_curve(y, 0.3)
    return [y, y, y]


def faded(c):
    # An old print: blacks lifted and a little blue, highlights yellowed, colors faded.
    c = saturate(c, 0.75)
    y = luma(c)
    c = [c[0] + 0.03 * y, c[1] + 0.02 * y, c[2] - 0.04 * y + 0.03 * (1 - y)]
    return levels(c, 0.07, 0.93)


LOOKS = {
    "warm-film": ("Warm film", warm_film),
    "teal-orange": ("Teal and orange", teal_orange),
    "bleach-bypass": ("Bleach bypass", bleach_bypass),
    "black-and-white": ("Black and white", black_and_white),
    "faded": ("Faded print", faded),
}

folder.mkdir(parents=True, exist_ok=True)
for name, (title, look) in LOOKS.items():
    lines = [f'TITLE "{title}"', "# Far Ray Tracer look, made by tools/generate_luts.py", f"LUT_3D_SIZE {SIZE}", ""]
    step = 1 / (SIZE - 1)
    for b in range(SIZE):
        for g in range(SIZE):
            for r in range(SIZE):
                out = look([r * step, g * step, b * step])
                lines.append(" ".join(f"{min(1.0, max(0.0, v)):.6f}" for v in out))
    target = folder / f"{name}.cube"
    target.write_text("\n".join(lines) + "\n")
    print(target)
