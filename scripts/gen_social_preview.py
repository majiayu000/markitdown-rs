"""Draw docs/social-preview.png (1280x640), the GitHub social preview and
the landing page's Open Graph image. Uses macOS system fonts.

usage: uv run --with pillow python scripts/gen_social_preview.py
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

OUT = Path(__file__).resolve().parent.parent / "docs/social-preview.png"
W, H = 1280, 640
BG, FG, DIM, ACCENT, CODE_BG = "#0d1117", "#e6edf3", "#8b949e", "#f0883e", "#161b22"
GREEN, BLUE = "#7ee787", "#79c0ff"

sans_bold = ImageFont.truetype("/System/Library/Fonts/Supplemental/Arial Bold.ttf", 76)
sans = ImageFont.truetype("/System/Library/Fonts/Supplemental/Arial.ttf", 32)
small = ImageFont.truetype("/System/Library/Fonts/Supplemental/Arial.ttf", 25)
mono = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", 24)

img = Image.new("RGB", (W, H), BG)
d = ImageDraw.Draw(img)
d.rectangle([0, 0, W, 10], fill=ACCENT)
d.text((72, 64), "markitdown-rs", font=sans_bold, fill=FG)
d.text((72, 166), "Microsoft's markitdown in one Rust binary.", font=sans, fill=FG)
d.text((72, 210), "Same Markdown, byte for byte. No Python. 23-112x faster.", font=sans, fill=DIM)

box = [72, 280, W - 72, 520]
d.rounded_rectangle(box, radius=14, fill=CODE_BG, outline="#30363d", width=2)
# real output: markitdown tests/fixtures/pdf/gen-form-mixed.pdf
lines = [
    [("$ ", DIM), ("markitdown order.pdf", FG)],
    [("Purchase order summary", FG)],
    [("| Item     | Qty | Unit price | Total |", GREEN)],
    [("| -------- | --- | ---------- | ----- |", GREEN)],
    [("| Widget 1 | 1   | 1.50       | 1.50  |", GREEN)],
    [("| Widget 2 | 2   | 3.00       | 6.00  |", GREEN)],
]
y = 300
for parts in lines:
    x = 100
    for text, color in parts:
        d.text((x, y), text, font=mono, fill=color)
        x += d.textlength(text, font=mono)
    y += 36

d.text(
    (72, 560),
    "PDF  DOCX  PPTX  XLSX  XLS  HTML  EPUB  MSG  CSV  ZIP  RSS  ipynb  URLs",
    font=small,
    fill=DIM,
)
img.save(OUT, optimize=True)
print(OUT, OUT.stat().st_size)
