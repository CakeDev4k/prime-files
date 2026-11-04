"""Draws the Prime Files icon and writes prime-files.ico and prime-files.png.

    python assets/icon/make_icon.py

A plain folder, with no mark on it. Drawn in code rather than kept only as
a picture so the shapes can be changed and the icon regenerated at every
size Windows asks for. Needs Pillow. Each size is drawn at 4x and scaled
down, which is what keeps the edges smooth at 16 px.
"""

from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]

BACK = (214, 120, 16, 255)   # the folder's back and tab, darker
FRONT = (247, 166, 36, 255)  # the folder's front


def draw(size: int) -> Image.Image:
    s = size * 4
    image = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(image)

    def box(x0, y0, x1, y1):
        return [round(x0 * s), round(y0 * s), round(x1 * s), round(y1 * s)]

    radius = round(0.07 * s)
    # The tab, then the back, then the front lower down, so a band of the
    # back shows above it.
    d.rounded_rectangle(box(0.06, 0.18, 0.48, 0.38), radius=radius, fill=BACK)
    d.rounded_rectangle(box(0.06, 0.26, 0.94, 0.86), radius=radius, fill=BACK)
    d.rounded_rectangle(box(0.06, 0.34, 0.94, 0.86), radius=radius, fill=FRONT)
    return image.resize((size, size), Image.LANCZOS)


def main() -> None:
    images = [draw(size) for size in SIZES]
    images[-1].save(HERE / "prime-files.ico", sizes=[(n, n) for n in SIZES], append_images=images[:-1])
    draw(512).save(HERE / "prime-files.png")


if __name__ == "__main__":
    main()
