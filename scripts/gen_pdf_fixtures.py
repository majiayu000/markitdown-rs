"""Write the generated PDF fixtures in tests/fixtures/pdf/gen-*.pdf.

Most are assembled by hand with a minimal PDF writer, so each exercises
one construct of pdfminer's parser, fonts or interpreter; reportlab makes
the standard-14 and CJK (predefined CMap) files, pikepdf the encrypted and
object-stream ones.

usage: uv run --with reportlab==5.0.1 --with pikepdf==10.17.0 \
           python scripts/gen_pdf_fixtures.py
"""

import base64
import io
import zlib
from pathlib import Path

import pikepdf
from reportlab.lib.pagesizes import letter
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.cidfonts import UnicodeCIDFont
from reportlab.pdfgen import canvas

OUT = Path(__file__).resolve().parent.parent / "tests/fixtures/pdf"


class Pdf:
    """Objects are numbered as they are added; 1 is the catalog and 2 the
    page tree."""

    def __init__(self):
        self.objs = [None, None]
        self.pages = []

    def add(self, body):
        self.objs.append(body if isinstance(body, bytes) else body.encode("latin-1"))
        return len(self.objs)

    def stream(self, data, extra=""):
        head = f"<< /Length {len(data)} {extra} >>\nstream\n".encode("latin-1")
        return self.add(head + data + b"\nendstream")

    def page(self, content, resources="", extra=""):
        if isinstance(content, (bytes, str)):
            content = [content]
        ids = []
        for c in content:
            if isinstance(c, int):
                ids.append(c)
            else:
                ids.append(self.stream(c.encode("latin-1") if isinstance(c, str) else c))
        contents = " ".join(f"{i} 0 R" for i in ids)
        self.pages.append(
            self.add(f"<< /Type /Page /Parent 2 0 R /Contents [{contents}] /Resources << {resources} >> {extra} >>")
        )

    def bytes(self, catalog_extra="", xref=True, startxref_shift=0):
        kids = " ".join(f"{p} 0 R" for p in self.pages)
        self.objs[0] = f"<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>".encode()
        self.objs[1] = f"<< /Type /Pages /Kids [{kids}] /Count {len(self.pages)} /MediaBox [0 0 612 792] >>".encode()
        out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
        offsets = []
        for i, body in enumerate(self.objs, 1):
            offsets.append(len(out))
            out += b"%d 0 obj\n" % i + body + b"\nendobj\n"
        pos = len(out)
        if xref:
            out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(self.objs) + 1)
            out += b"".join(b"%010d 00000 n \n" % o for o in offsets)
        out += b"trailer\n<< /Size %d /Root 1 0 R >>\n" % (len(self.objs) + 1)
        out += b"startxref\n%d\n%%%%EOF\n" % (pos + startxref_shift)
        return bytes(out)


HELV = "/Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >>"


def text_page(pdf, lines, extra=""):
    ops = ["BT /F1 12 Tf 72 720 Td 14 TL"]
    for line in lines:
        ops.append(f"({line}) Tj T*")
    ops.append("ET")
    pdf.page("\n".join(ops), HELV, extra)


def write(name, data):
    (OUT / name).write_bytes(data)


def simple_doc(lines, **kw):
    pdf = Pdf()
    text_page(pdf, lines)
    return pdf.bytes(**kw)


def standard14():
    buf = io.BytesIO()
    c = canvas.Canvas(buf, pagesize=letter, invariant=1)
    y = 740
    sample = "Café naïve façade — “quotes” £5 ½ ß"
    for face in sorted(set(canvas.Canvas(io.BytesIO()).getAvailableFonts())):
        c.setFont(face, 11)
        c.drawString(72, y, f"{face}: " + ("αβγ ∑ ∞" if face == "Symbol" else "✓✗☎ ❤" if face == "ZapfDingbats" else sample))
        y -= 18
    c.showPage()
    c.save()
    write("gen-standard14.pdf", buf.getvalue())


def cjk():
    buf = io.BytesIO()
    c = canvas.Canvas(buf, pagesize=letter, invariant=1)
    y = 720
    for face, text in [
        ("STSong-Light", "简体中文：数据转换为文本。"),
        ("MSung-Light", "繁體中文：資料轉換為文字。"),
        ("HeiseiMin-W3", "日本語のテキスト、かな・カナ。"),
        ("HeiseiKakuGo-W5", "ゴシック体の見出し"),
        ("HYSMyeongJo-Medium", "한국어 텍스트 변환"),
    ]:
        pdfmetrics.registerFont(UnicodeCIDFont(face))
        c.setFont(face, 14)
        c.drawString(72, y, text)
        y -= 30
    vertical = UnicodeCIDFont("HeiseiMin-W3", isVertical=True)
    vertical.name = vertical.fontName = "HeiseiMin-W3-V"
    pdfmetrics.registerFont(vertical)
    c.setFont(vertical.fontName, 14)
    c.drawString(400, 600, "縦書きの文章")
    c.showPage()
    c.save()
    write("gen-cjk.pdf", buf.getvalue())


def encrypted():
    base = simple_doc(["Encrypted document text.", "Second line (with parens) and \\\\ backslash."])
    for name, enc in [
        ("rc4-40", pikepdf.Encryption(owner="owner", user="", R=2, metadata=False, aes=False)),
        ("rc4-128", pikepdf.Encryption(owner="owner", user="", R=3, metadata=False, aes=False)),
        ("aes-128", pikepdf.Encryption(owner="owner", user="", R=4, aes=True)),
        ("aes-128-meta", pikepdf.Encryption(owner="owner", user="", R=4, aes=True, metadata=False)),
        ("aes-256", pikepdf.Encryption(owner="owner", user="", R=6, aes=True)),
    ]:
        with pikepdf.open(io.BytesIO(base)) as p:
            p.docinfo["/Title"] = "Secret title"
            out = io.BytesIO()
            p.save(out, encryption=enc, static_id=True)
        write(f"gen-encrypt-{name}.pdf", out.getvalue())


def objstm():
    base = simple_doc(["Objects packed in object streams,", "with a cross-reference stream."])
    with pikepdf.open(io.BytesIO(base)) as p:
        out = io.BytesIO()
        p.save(out, object_stream_mode=pikepdf.ObjectStreamMode.generate, deterministic_id=True)
    write("gen-objstm.pdf", out.getvalue())


def lzw_encode(data):
    table = {bytes([i]): i for i in range(256)}
    bits, nbits, out, w = [], 9, bytearray(), b""
    codes = [256]
    for c in data:
        wc = w + bytes([c])
        if wc in table:
            w = wc
            continue
        codes.append(table[w])
        table[wc] = len(table) + 2
        w = bytes([c])
    if w:
        codes.append(table[w])
    codes.append(257)
    size = 258
    for i, code in enumerate(codes):
        bits.append((code, nbits))
        if i > 0 and code not in (256, 257):
            size += 1
            if size == 511:
                nbits = 10
            elif size == 1023:
                nbits = 11
            elif size == 2047:
                nbits = 12
    acc, n = 0, 0
    for code, w_ in bits:
        acc = (acc << w_) | code
        n += w_
        while n >= 8:
            out.append((acc >> (n - 8)) & 0xFF)
            n -= 8
    if n:
        out.append((acc << (8 - n)) & 0xFF)
    return bytes(out)


def rle_encode(data):
    out = bytearray()
    i = 0
    while i < len(data):
        run = 1
        while i + run < len(data) and run < 128 and data[i + run] == data[i]:
            run += 1
        if run > 1:
            out += bytes([257 - run, data[i]])
            i += run
        else:
            j = i
            while j < len(data) and j - i < 128 and (j + 1 >= len(data) or data[j + 1] != data[j]):
                j += 1
            out += bytes([j - i - 1]) + data[i:j]
            i = j
    return bytes(out + b"\x80")


def png_rows(data, columns):
    rows = [data[i : i + columns] for i in range(0, len(data), columns)]
    out, prev = bytearray(), bytes(columns)
    for n, row in enumerate(rows):
        row = row.ljust(columns, b" ")
        kind = n % 3  # none, sub, up
        if kind == 0:
            out += b"\x00" + row
        elif kind == 1:
            out += b"\x01" + bytes((row[i] - (row[i - 1] if i else 0)) & 255 for i in range(columns))
        else:
            out += b"\x02" + bytes((row[i] - prev[i]) & 255 for i in range(columns))
        prev = row
    return bytes(out)


def filters():
    pdf = Pdf()

    def content(label):
        return f"BT /F1 12 Tf 72 700 Td (Filter: {label}) Tj 0 -16 Td (The quick brown fox jumps over the lazy dog.) Tj ET".encode()

    for label, enc, extra in [
        ("ASCIIHexDecode", lambda d: d.hex().encode() + b">", "/Filter /ASCIIHexDecode"),
        ("ASCII85Decode", lambda d: base64.a85encode(d, adobe=True), "/Filter /A85"),
        ("LZWDecode", lzw_encode, "/Filter /LZWDecode"),
        ("RunLengthDecode", rle_encode, "/Filter /RunLengthDecode"),
        ("A85 then Flate", lambda d: base64.a85encode(zlib.compress(d), adobe=True), "/Filter [/ASCII85Decode /FlateDecode]"),
    ]:
        sid = pdf.stream(enc(content(label)), extra)
        pdf.page([sid], HELV)
    raw = content("Flate PNG predictor")
    sid = pdf.stream(zlib.compress(png_rows(raw, 16)), "/Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 16 >>")
    pdf.page([sid], HELV)
    # one page drawn from three streams, a string and an operator split
    # across the stream boundaries
    pdf.page(["BT /F1 12 Tf 72 700 Td (split acr", "oss streams) Tj 0 -16 T", "d (next line) Tj ET"], HELV)
    write("gen-filters.pdf", pdf.bytes())


TOUNICODE = b"""/CIDInit /ProcSet findresource begin 12 dict begin begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Adobe-Identity-UCS def /CMapType 2 def
1 begincodespacerange <0000> <FFFF> endcodespacerange
3 beginbfchar <0001> <0048> <0002> <0069> <0003> <FB01>
endbfchar
2 beginbfrange <0010> <0019> <0030> <0020> <0022> [<00E9> <00E8> <D83DDE00>]
endbfrange
1 begincidrange <0040> <0045> 65 endcidrange
endcmap CMapName currentdict /CMap defineresource pop end end"""


def cid_fonts():
    pdf = Pdf()
    tu = pdf.stream(TOUNICODE)
    desc = pdf.add("<< /Type /FontDescriptor /FontName /Fake /Flags 4 /FontBBox [0 -200 1000 800] /Ascent 800 /Descent -200 >>")
    cid = pdf.add(f"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Fake /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {desc} 0 R /DW 600 /W [1 [700 300] 16 25 550 32 [400 400 900]] >>")
    t0 = pdf.add(f"<< /Type /Font /Subtype /Type0 /BaseFont /Fake /Encoding /Identity-H /DescendantFonts [{cid} 0 R] /ToUnicode {tu} 0 R >>")
    t0_nomap = pdf.add(f"<< /Type /Font /Subtype /Type0 /BaseFont /Fake /Encoding /Identity-H /DescendantFonts [{cid} 0 R] >>")
    cidv = pdf.add(f"<< /Type /Font /Subtype /CIDFontType0 /BaseFont /FakeV /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {desc} 0 R /W2 [1 [-1000 500 880]] /DW2 [880 -1000] >>")
    t0v = pdf.add(f"<< /Type /Font /Subtype /Type0 /BaseFont /FakeV /Encoding /Identity-V /DescendantFonts [{cidv} 0 R] /ToUnicode /Identity-H >>")
    res = f"/Font << /T0 {t0} 0 R /T1 {t0_nomap} 0 R /TV {t0v} 0 R >>"
    pdf.page(
        "BT /T0 14 Tf 72 700 Td <000100020003> Tj 0 -20 Td <00100011001200130020002100220040004100420045> Tj "
        "0 -20 Td [<0001> -250 <0002>] TJ ET BT /T1 14 Tf 72 600 Td <00010002> Tj ET "
        "BT /TV 14 Tf 400 700 Td <0041004200430044> Tj ET",
        res,
    )
    write("gen-cid-fonts.pdf", pdf.bytes())


def type3():
    pdf = Pdf()
    glyph = pdf.stream(b"1000 0 0 0 750 750 d1 0 0 750 750 re f")
    font = pdf.add(
        f"<< /Type /Font /Subtype /Type3 /FontBBox [0 0 750 750] /FontMatrix [0.001 0 0 0.001 0 0] "
        f"/CharProcs << /square {glyph} 0 R >> /Encoding << /Type /Encoding /Differences [65 /H /e /l /l.alt /o /space /square /uni2603 /u1F600 /f_i] >> "
        "/FirstChar 65 /LastChar 74 /Widths [700 550 300 300 600 300 1000 900 1000 550] >>"
    )
    pdf.page("BT /F3 14 Tf 72 700 Td (ABCDEFGHIJ) Tj 0 -20 Td 2 Tz (ABCDE) Tj ET", f"/Font << /F3 {font} 0 R >>")
    write("gen-type3.pdf", pdf.bytes())


def encodings():
    pdf = Pdf()
    fonts = {
        "W": "/BaseFont /Custom-Win /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 255 /Widths [" + " ".join(["500"] * 224) + "]",
        "M": "/BaseFont /Custom-Mac /Encoding << /BaseEncoding /MacRomanEncoding /Differences [128 /Euro /uni00A9 /u2122 /f_f_i /A.sc] >>",
        "P": "/BaseFont /Custom-Pdf /Encoding /PDFDocEncoding",
        "S": "/BaseFont /Times-Roman /Encoding << /Differences [39 /quotesingle 96 /grave] >>",
        "T": "/Subtype /TrueType /BaseFont /Arial,Bold",
        "U": "/BaseFont /NoMetrics",
    }
    res = "/Font << " + " ".join(
        f"/{k} << /Type /Font {'/Subtype /Type1 ' if '/Subtype' not in v else ''}{v} >>" for k, v in fonts.items()
    ) + " >>"
    ops = ["BT 72 720 Td 16 TL"]
    for k in fonts:
        ops.append(f"/{k} 11 Tf (\\200\\201\\202\\203\\204 \\251\\256\\337 \\344\\366\\374 '` caf\\351) Tj T*")
    ops.append("ET")
    pdf.page("\n".join(ops), res)
    write("gen-encodings.pdf", pdf.bytes())


def text_state():
    pdf = Pdf()
    pdf.page(
        "BT /F1 12 Tf 72 720 Td 14 TL (Plain line) Tj T* 3 Tc (Spaced letters) Tj T* 0 Tc 10 Tw (Wide word spacing here) Tj "
        "T* 0 Tw 50 Tz (Condensed) Tj 100 Tz T* (apostrophe op) ' 5 2 (quote op) \" T* 0 0 Tc 0 Tw "
        "[(Kern) -120 (ing) 300 (TJ) -2000 (gap)] TJ T* 4 Ts (risen) Tj 0 Ts ( text) Tj ET "
        "BT /F1 12 Tf 0 1 -1 0 300 400 Tm (Rotated ninety) Tj ET "
        "BT /F1 12 Tf -1 0 0 -1 500 300 Tm (Upside down) Tj ET "
        "BT /F1 12 Tf 1 0 0 1 72 200 Tm (A) Tj ( B) Tj (C D) Tj ET",
        HELV,
    )
    write("gen-text-state.pdf", pdf.bytes())


def xobjects():
    pdf = Pdf()
    inner = pdf.stream(b"BT /F1 10 Tf 0 0 Td (Nested form text) Tj ET", f"/Type /XObject /Subtype /Form /BBox [0 0 300 50] /Resources << {HELV} >>")
    form = pdf.stream(
        b"BT /F1 12 Tf 0 30 Td (Form XObject text) Tj ET q 1 0 0 1 0 -40 cm /Inner Do Q",
        f"/Type /XObject /Subtype /Form /BBox [0 0 300 100] /Matrix [1 0 0 1 0 0] /Resources << {HELV} /XObject << /Inner {inner} 0 R >> >>",
    )
    noresources = pdf.stream(b"BT /F1 12 Tf 0 0 Td (Form using page resources) Tj ET", "/Type /XObject /Subtype /Form /BBox [0 0 300 20]")
    image = pdf.stream(b"\xff\x00\xff\x00", "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8")
    res = f"{HELV} /XObject << /Fm {form} 0 R /Fm2 {noresources} 0 R /Im {image} 0 R >>"
    content = (
        b"BT /F1 12 Tf 72 720 Td (Before the forms) Tj ET q 1 0 0 1 72 600 cm /Fm Do Q "
        b"q 1 0 0 1 72 500 cm /Fm2 Do Q q 20 0 0 20 72 450 cm /Im Do Q "
        b"BI /W 4 /H 1 /CS /G /BPC 8 ID \x00EI\xffEI\n EI\n"
        b"BT /F1 12 Tf 72 400 Td (After an inline image) Tj ET "
        b"q 2 0 0 2 0 0 cm /Fm Do Q BT /F1 12 Tf 72 300 Td (Text after a scaled form) Tj ET"
    )
    pdf.page(content, res)
    write("gen-xobjects.pdf", pdf.bytes())


def rotation():
    pdf = Pdf()
    for rot in (0, 90, 180, 270, -90):
        text_page(pdf, [f"Rotate {rot} page", "second line"], f"/Rotate {rot}")
    text_page(pdf, ["Offset media box"], "/MediaBox [100 100 712 892] /CropBox [110 110 700 880]")
    write("gen-rotate.pdf", pdf.bytes())


def xref_variants():
    write("gen-broken-startxref.pdf", simple_doc(["startxref points nowhere"], startxref_shift=17))
    write("gen-no-xref.pdf", simple_doc(["no xref table at all"], xref=False))
    base = simple_doc(["First revision"])
    # incremental update replacing the page's content stream
    new_content = b"BT /F1 12 Tf 72 720 Td (Second revision text) Tj ET"
    obj = b"3 0 obj\n<< /Length %d >>\nstream\n" % len(new_content) + new_content + b"\nendstream\nendobj\n"
    prev = int(base.rsplit(b"startxref\n", 1)[1].split(b"\n")[0])
    pos = len(base)
    xref_pos = pos + len(obj)
    update = obj + b"xref\n3 1\n%010d 00000 n \ntrailer\n<< /Size 5 /Root 1 0 R /Prev %d >>\nstartxref\n%d\n%%%%EOF\n" % (pos, prev, xref_pos)
    write("gen-incremental.pdf", base + update)


def page_labels():
    pdf = Pdf()
    for i in range(4):
        text_page(pdf, [f"Labelled page {i + 1}"])
    write("gen-page-labels.pdf", pdf.bytes("/PageLabels << /Nums [0 << /S /r >> 2 << /S /A /P (App-) /St 3 >>] >>"))


def form_mixed():
    buf = io.BytesIO()
    c = canvas.Canvas(buf, pagesize=letter, invariant=1)
    c.setFont("Helvetica", 10)
    c.drawString(72, 740, "Purchase order summary")
    rows = [("Item", "Qty", "Unit price", "Total")] + [(f"Widget {i}", str(i), f"{i * 1.5:.2f}", f"{i * i * 1.5:.2f}") for i in range(1, 9)]
    y = 700
    for row in rows:
        for x, cell in zip((72, 250, 350, 470), row):
            c.drawString(x, y, cell)
        y -= 16
    c.drawString(72, y - 20, "Thank you for your order.")
    c.showPage()
    c.setFont("Times-Roman", 12)
    for i, line in enumerate(["An ordinary page of prose follows the table.", "It has no columns, so pdfplumber's text is used."]):
        c.drawString(72, 700 - 16 * i, line)
    c.showPage()
    c.save()
    write("gen-form-mixed.pdf", buf.getvalue())


def empty():
    pdf = Pdf()
    write("gen-no-pages.pdf", pdf.bytes())
    pdf = Pdf()
    pdf.page(b"", "")
    write("gen-blank-page.pdf", pdf.bytes())


for f in (standard14, cjk, encrypted, objstm, filters, cid_fonts, type3, encodings, text_state, xobjects, rotation, xref_variants, page_labels, form_mixed, empty):
    f()
