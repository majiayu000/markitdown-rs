"""Write the hand-built .docx fixtures in tests/fixtures/docx/gen-*.docx.

Each file targets DOCX behaviour that real documents hit and that upstream
markitdown issues report (issue numbers in the comments). The XML is written
directly so every construct is exact and the output is deterministic.

usage: uv run --project oracle python scripts/gen_fixtures.py
"""

import zipfile
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "tests/fixtures/docx"

NS = (
    'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" '
    'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" '
    'xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" '
    'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
    'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
    'xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" '
    'xmlns:v="urn:schemas-microsoft-com:vml" '
    'xmlns:o="urn:schemas-microsoft-com:office:office" '
    'xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" '
    'xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" '
    'xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" '
    'mc:Ignorable="w14"'
)
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'

# 1x1 transparent PNG
PNG = bytes.fromhex(
    "89504e470d0a1a0a0000000d4948445200000001000000010806000000"
    "1f15c4890000000d49444154789c6360000002000154a24f5d0000000049454e44ae426082"
)


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace('"', "&quot;")


def t(text):
    return f'<w:t xml:space="preserve">{esc(text)}</w:t>'


def r(text="", props="", extra=""):
    rpr = f"<w:rPr>{props}</w:rPr>" if props else ""
    return f"<w:r>{rpr}{t(text) if text else ''}{extra}</w:r>"


def p(*runs, style=None, num=None, ppr=""):
    props = ppr
    if style:
        props = f'<w:pStyle w:val="{style}"/>' + props
    if num is not None:
        num_id, ilvl = num
        lvl = "" if ilvl is None else f'<w:ilvl w:val="{ilvl}"/>'
        props += f'<w:numPr>{lvl}<w:numId w:val="{num_id}"/></w:numPr>'
    ppr_xml = f"<w:pPr>{props}</w:pPr>" if props else ""
    body = "".join(x if x.startswith("<") else r(x) for x in runs)
    return f"<w:p>{ppr_xml}{body}</w:p>"


def tc(*paras, props=""):
    return f"<w:tc><w:tcPr>{props}</w:tcPr>{''.join(paras) or '<w:p/>'}</w:tc>"


def tr(*cells, props=""):
    trpr = f"<w:trPr>{props}</w:trPr>" if props else ""
    return f"<w:tr>{trpr}{''.join(cells)}</w:tr>"


def tbl(*rows):
    return f'<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/></w:tblPr>{"".join(rows)}</w:tbl>'


STYLES = [
    ("paragraph", "Normal", "Normal"),
    ("paragraph", "Title", "Title"),
    ("paragraph", "ListParagraph", "List Paragraph"),
    ("paragraph", "ListBullet", "List Bullet"),
    ("paragraph", "FootnoteText", "footnote text"),
    ("character", "FootnoteReference", "footnote reference"),
    ("character", "Strong", "Strong"),
    ("character", "Hyperlink", "Hyperlink"),
    ("table", "TableGrid", "Table Grid"),
] + [("paragraph", f"Heading{i}", f"heading {i}") for i in range(1, 10)]


def styles_xml(styles=STYLES, extra=""):
    body = "".join(
        f'<w:style w:type="{typ}" w:styleId="{sid}"><w:name w:val="{name}"/></w:style>'
        for typ, sid, name in styles
    )
    return f"{DECL}<w:styles {NS}>{body}{extra}</w:styles>"


def numbering_xml(abstracts, nums):
    """abstracts: {abstract_id: [(ilvl, numFmt, pStyle|None), ...]}, nums: {num_id: abstract_id}"""
    out = []
    for aid, levels in abstracts.items():
        lv = "".join(
            f'<w:lvl{"" if i is None else f" w:ilvl=\"{i}\""}><w:start w:val="1"/>'
            f'{f"<w:numFmt w:val=\"{fmt}\"/>" if fmt else ""}'
            f'{f"<w:pStyle w:val=\"{ps}\"/>" if ps else ""}</w:lvl>'
            for i, fmt, ps in levels
        )
        out.append(f'<w:abstractNum w:abstractNumId="{aid}">{lv}</w:abstractNum>')
    for nid, aid in nums.items():
        out.append(f'<w:num w:numId="{nid}"><w:abstractNumId w:val="{aid}"/></w:num>')
    return f"{DECL}<w:numbering {NS}>{''.join(out)}</w:numbering>"


def write_docx(name, body, *, styles=None, numbering=None, rels=(), parts=None, names=None):
    """rels: [(id, type, target, external?)]; parts: {zip name: bytes|str}"""
    parts = dict(parts or {})
    doc_rels = [
        ("rIdStyles", "styles", "styles.xml", False),
    ] + list(rels)
    if numbering:
        doc_rels.append(("rIdNumbering", "numbering", "numbering.xml", False))
        parts["word/numbering.xml"] = numbering
    for zip_name, kind in (("word/footnotes.xml", "footnotes"), ("word/endnotes.xml", "endnotes"),
                           ("word/comments.xml", "comments")):
        if zip_name in parts:
            doc_rels.append((f"rId{kind}", kind, zip_name.split("/")[1], False))
    rel_xml = "".join(
        f'<Relationship Id="{i}" Type="{REL}/{typ}" Target="{esc(target)}"'
        f'{" TargetMode=\"External\"" if ext else ""}/>'
        for i, typ, target, ext in doc_rels
    )
    files = {
        "[Content_Types].xml": (
            f'{DECL}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
            '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
            '<Default Extension="xml" ContentType="application/xml"/>'
            '<Default Extension="png" ContentType="image/png"/>'
            '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
            "</Types>"
        ),
        "_rels/.rels": (
            f'{DECL}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            f'<Relationship Id="rId1" Type="{REL}/officeDocument" Target="word/document.xml"/></Relationships>'
        ),
        "word/_rels/document.xml.rels": (
            f'{DECL}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            f"{rel_xml}</Relationships>"
        ),
        "word/document.xml": f"{DECL}<w:document {NS}><w:body>{body}<w:sectPr/></w:body></w:document>",
        "word/styles.xml": styles or styles_xml(),
    }
    files.update(parts)
    with zipfile.ZipFile(OUT / name, "w", zipfile.ZIP_DEFLATED) as z:
        for zname, data in files.items():
            info = zipfile.ZipInfo((names or {}).get(zname, zname), date_time=(2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, data)


def lists():
    num = numbering_xml(
        {
            0: [(i, "bullet", None) for i in range(9)],
            1: [(i, "decimal", None) for i in range(9)],
            2: [(0, "bullet", "ListBullet")],  # list via paragraph style
            3: [(None, "lowerLetter", None)],  # level without w:ilvl
            4: [(0, "decimal", None), (1, "bullet", None), (2, "decimal", None)],
        },
        {1: 0, 2: 1, 3: 2, 4: 3, 5: 4, 6: 1},
    )
    body = "".join([
        p("Bullets nested five deep, then beyond mammoth's five levels:"),
        *[p(f"bullet level {i}", num=(1, i)) for i in range(7)],
        p("Back to text. Numbered list, restarting after text:"),
        p("one", num=(2, 0)), p("two", num=(2, 0)),
        p("two.a", num=(2, 1)), p("two.a.i", num=(2, 2)), p("three", num=(2, 0)),
        p("Mixed kinds at the same level split the list (issue 2323):"),
        p("ordered", num=(5, 0)), p("bullet child", num=(5, 1)), p("ordered grandchild", num=(5, 2)),
        p("bullet at top", num=(1, 0)), p("ordered at top", num=(2, 0)),
        p("Style-based bullets:"),
        p("styled bullet", style="ListBullet"), p("styled bullet 2", style="ListBullet"),
        p("numId without ilvl (issue 1282):"),
        p("no ilvl one", num=(4, None)), p("no ilvl two", num=(4, None)),
        p("numId 0 means not a list:"),
        p("not a list item", num=(0, 0)),
        p("Unknown numId:"),
        p("unknown num", num=(99, 0)),
        p("Second instance of the decimal list:"),
        p("again one", num=(6, 0)),
        p("List items with formatting: ", ),
        p(r("bold item", "<w:b/>"), " and ", r("italic", "<w:i/>"), num=(1, 0)),
        p("item with line", r("", "", "<w:br/>"), "break", num=(1, 0)),
        p("", num=(1, 0)),
        p("after an empty item", num=(1, 0)),
        p("Heading styled list paragraph:"),
        p("numbered heading", style="Heading2", num=(2, 0)),
    ])
    write_docx("gen-lists.docx", body, numbering=num)


def headings():
    localized = STYLES + [("paragraph", "berschrift1", "Überschrift 1"), ("paragraph", "1", "heading 1"),
                          ("paragraph", "Heading", "Heading"), ("paragraph", "CustomH", "Heading 3")]
    body = "".join([
        p("Document title", style="Title"),
        *[p(f"Heading level {i}", style=f"Heading{i}") for i in range(1, 10)],  # 7-9: issue 2530
        p("German style name", style="berschrift1"),
        p("Numeric style id named heading 1", style="1"),
        p("Apple Pages Heading", style="Heading"),
        p("Custom id, name Heading 3", style="CustomH"),
        p("Undefined style id", style="NoSuchStyle"),
        p(r("Bold "), r("heading", "<w:b/>"), " text", style="Heading2"),
        p("Heading  with\ttab and   spaces", style="Heading3"),
        p("# not a heading, 1. not a list, * not bullet, _under_, a*b*c, `code`, [x](y), <tag> & 50%"),
        p("", style="Heading1"),
        p("Heading with break", r("", "", "<w:br/>"), "second line", style="Heading1"),
    ])
    write_docx("gen-headings.docx", body, styles=styles_xml(localized))


def tables():
    span = '<w:gridSpan w:val="2"/>'
    vstart = '<w:vMerge w:val="restart"/>'
    vcont = "<w:vMerge/>"
    nested = tbl(tr(tc(p("inner a")), tc(p("inner b"))), tr(tc(p("inner c")), tc(p("inner d"))))
    body = "".join([
        p("Header row repeated (tblHeader):"),
        tbl(
            tr(tc(p("Name")), tc(p("Value")), props="<w:tblHeader/>"),
            tr(tc(p("alpha")), tc(p("1"))),
            tr(tc(p("beta")), tc(p("2"))),
        ),
        p("Merged cells:"),
        tbl(
            tr(tc(p("spans two"), props=span), tc(p("c"))),
            tr(tc(p("tall"), props=vstart), tc(p("x")), tc(p("y"))),
            tr(tc(p("hidden"), props=vcont), tc(p("z")), tc(p("w"))),
        ),
        p("Nested table (issue 1248):"),
        tbl(tr(tc(p("outer")), tc(p("before"), nested, p("after")))),
        p("Pipes, breaks and paragraphs in cells (issue 2438):"),
        tbl(
            tr(tc(p("a | b")), tc(p("line one", r("", "", "<w:br/>"), "line two"))),
            tr(tc(p("para one"), p("para two")), tc()),
            tr(tc(p(r("bold", "<w:b/>"), " cell")), tc(p("list in cell", num=None))),
        ),
        p("All header rows:"),
        tbl(tr(tc(p("only")), tc(p("header")), props="<w:tblHeader/>")),
        p("Deleted row is dropped:"),
        tbl(tr(tc(p("kept"))), tr(tc(p("deleted")), props='<w:del w:id="1" w:author="x" w:date="2026-01-01T00:00:00Z"/>')),
        p("Ragged rows:"),
        tbl(tr(tc(p("1")), tc(p("2")), tc(p("3"))), tr(tc(p("only one")))),
        p("Bookmark between rows disables merging:"),
        f'<w:tbl><w:tblPr/>{tr(tc(p("a"), props=vstart))}<w:bookmarkStart w:id="0" w:name="mid"/>'
        f'{tr(tc(p("b"), props=vcont))}</w:tbl>',
        p("Empty table:"),
        "<w:tbl><w:tblPr/></w:tbl>",
        p("End."),
    ])
    write_docx("gen-tables.docx", body)


def inline():
    body = "".join([
        p(r("bold", "<w:b/>"), " ", r("italic", "<w:i/>"), " ", r("both", "<w:b/><w:i/>"), " ",
          r("underline", '<w:u w:val="single"/>'), " ", r("no-val underline", "<w:u/>"), " ",
          r("strike", "<w:strike/>"), " ", r("double strike", "<w:dstrike/>"), " ",
          r("super", '<w:vertAlign w:val="superscript"/>'), " ", r("sub", '<w:vertAlign w:val="subscript"/>')),
        p(r("off", '<w:b w:val="0"/>'), " ", r("false", '<w:i w:val="false"/>'), " ",
          r("caps", "<w:caps/>"), " ", r("highlight", '<w:highlight w:val="yellow"/>'), " ",
          r("strong style", '<w:rStyle w:val="Strong"/>')),
        # formatting boundaries mid-word and around spaces (issue 1539)
        p(r("hel"), r("lo", "<w:b/>"), r(" wor", "<w:b/>"), r("ld "), r(" spaced ", "<w:i/>"), r("end")),
        p(r("bold ", "<w:b/>"), r("then bold italic", "<w:b/><w:i/>"), r(" then plain")),
        # smart quotes and dashes (issue 1219), escaping (issue 2157)
        p("“smart” ‘quotes’ — dash – en … ellipsis"),
        p("Escapes: *star* _under_ __dunder__ ** \\backslash `tick` #hash <angle> & amp [b]"),
        p("Tabs:", r("", "", "<w:tab/>"), "after tab", r("", "", "<w:tab/><w:tab/>"), "two tabs"),
        p("Line", r("", "", "<w:br/>"), "break and page", r("", "", '<w:br w:type="page"/>'), "break",
          r("", "", '<w:br w:type="column"/>'), "column"),
        p("Special: no", r("", "", "<w:noBreakHyphen/>"), "break soft", r("", "", "<w:softHyphen/>"), "hyphen"),
        p("Symbols:", r("", "", '<w:sym w:font="Wingdings" w:char="F0E0"/>'),
          r("", "", '<w:sym w:font="Symbol" w:char="F061"/>'), r("", "", '<w:sym w:font="Nope" w:char="0041"/>')),
        p(" nbsp inside  and trailing spaces   "),
        p("x" * 400),  # very long word (issue 1488)
        p("中文段落，包含标点。日本語のテキスト。한국어 텍스트."),
        p("RTL: שלום مرحبا and emoji \U0001F600\U0001F680"),
        p("   "),
        p(r("   ", "<w:b/>")),
        p(r("", "", '<w:t xml:space="preserve">entity &amp; &lt;tag&gt; &#169; &#x2603;</w:t>')),
        p("Deleted text is dropped: ", '<w:del w:id="2" w:author="x"><w:r><w:delText>gone</w:delText></w:r></w:del>',
          '<w:ins w:id="3" w:author="x"><w:r><w:t>inserted</w:t></w:r></w:ins>'),
        "<w:p><w:pPr><w:rPr><w:del w:id=\"4\" w:author=\"x\"/></w:rPr></w:pPr><w:r><w:t>deleted paragraph mark joins </w:t></w:r></w:p>",
        p("the next paragraph"),
        p('<w:smartTag w:uri="x" w:element="y"><w:r><w:t>smart tag text</w:t></w:r></w:smartTag>',
          '<w:customXml w:element="z"><w:r><w:t>custom xml is dropped</w:t></w:r></w:customXml>',
          '<w:fldSimple w:instr="PAGE"><w:r><w:t>simple field dropped</w:t></w:r></w:fldSimple>'),
        p(r("caps lock", "<w:smallCaps/>"), " ", r("sup bold", '<w:b/><w:vertAlign w:val="superscript"/>')),
    ])
    write_docx("gen-inline.docx", body)


def links():
    def fld(instr, text, props=""):
        return (f'<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve">{esc(instr)}</w:instrText></w:r>'
                f'<w:r><w:fldChar w:fldCharType="separate"/></w:r>{r(text, props)}{r(" more", props)}'
                f'<w:r><w:fldChar w:fldCharType="end"/></w:r>')

    rels = [
        ("rIdL1", "hyperlink", "https://example.com/path with space/ü?q=1&x=y#frag", True),
        ("rIdL2", "hyperlink", "mailto:someone@example.com", True),
        ("rIdL3", "hyperlink", "javascript:alert(1)", True),
        ("rIdL4", "hyperlink", "https://example.com/already%20encoded/%E4%B8%AD", True),
        ("rIdL5", "hyperlink", "HTTPS://Example.com/UPPER", True),
        ("rIdL6", "hyperlink", "https://example.com/a_b", True),
        ("rIdL7", "hyperlink", "file:///tmp/local file.txt", True),
        ("rIdL8", "hyperlink", "relative/path.html", True),
        ("rIdL9", "hyperlink", "https://example.com/page#old", True),
    ]

    def hl(rid, text, props="", anchor=None):
        a = f' w:anchor="{anchor}"' if anchor else ""
        rid_attr = f' r:id="{rid}"' if rid else ""
        return f"<w:hyperlink{rid_attr}{a}>{r(text, props)}</w:hyperlink>"

    body = "".join([
        p("External: ", hl("rIdL1", "spaces and unicode")),
        p("Mail: ", hl("rIdL2", "write to us"), " Script: ", hl("rIdL3", "dropped scheme")),
        p("Encoded: ", hl("rIdL4", "keep escapes"), " Upper scheme: ", hl("rIdL5", "upper")),
        p("Autolink: ", hl("rIdL6", "https://example.com/a_b")),
        p("File: ", hl("rIdL7", "local file"), " Relative: ", hl("rIdL8", "relative")),
        p("Anchor replaces fragment: ", hl("rIdL9", "new fragment", anchor="sec")),
        p("Internal TOC link (issue 2125): ", hl(None, "Section 1", anchor="_Toc123456")),
        p("Bold link: ", "<w:hyperlink r:id=\"rIdL6\">" + r("bold ", "<w:b/>") + r("part", "<w:b/>") + r(" plain") + "</w:hyperlink>"),
        p("Hyperlink style run: ", hl("rIdL6", "styled", '<w:rStyle w:val="Hyperlink"/>')),
        p("Field link: ", fld(' HYPERLINK "https://example.com/field" ', "field text")),
        p("Field anchor: ", fld(' HYPERLINK \\l "_Ref1" ', "ref text", "<w:b/>")),
        p("Other field: ", fld(" PAGEREF _Toc1 \\h ", "12")),
        p("Bookmarks: ", '<w:bookmarkStart w:id="1" w:name="target"/>', r("marked"), '<w:bookmarkEnd w:id="1"/>',
          '<w:bookmarkStart w:id="2" w:name="_GoBack"/>'),
        p("Empty link: ", hl("rIdL6", "")),
        p("Target frame: ", '<w:hyperlink r:id="rIdL6" w:tgtFrame="_blank"><w:r><w:t>new window</w:t></w:r></w:hyperlink>'),
        p("Checkbox field: ",
          '<w:r><w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:default w:val="1"/></w:checkBox></w:ffData></w:fldChar></w:r>'
          '<w:r><w:instrText> FORMCHECKBOX </w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>', " checked"),
        p("Content control checkbox: ",
          '<w:sdt><w:sdtPr><w14:checkbox><w14:checked w14:val="0"/></w14:checkbox></w:sdtPr>'
          '<w:sdtContent><w:r><w:t>☐</w:t></w:r></w:sdtContent></w:sdt>', " unchecked"),
        p("Plain content control: ", '<w:sdt><w:sdtPr/><w:sdtContent><w:r><w:t>inside sdt</w:t></w:r></w:sdtContent></w:sdt>'),
    ])
    write_docx("gen-links.docx", body, rels=rels)


def notes():
    def note_part(kind, notes):
        items = "".join(
            f'<w:{kind} w:id="{nid}"><w:p><w:pPr><w:pStyle w:val="FootnoteText"/></w:pPr>'
            f'<w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><w:{kind}Ref/></w:r>{body}</w:p></w:{kind}>'
            for nid, body in notes
        )
        sep = f'<w:{kind} w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:{kind}>'
        return f"{DECL}<w:{kind}s {NS}>{sep}{items}</w:{kind}s>"

    footnotes = note_part("footnote", [
        (1, r(" First footnote with ") + r("bold", "<w:b/>") + r(".")),
        (2, r(" Second footnote, ") + '<w:hyperlink r:id="rIdF1"><w:r><w:t>with a link</w:t></w:r></w:hyperlink>'),
    ])
    endnotes = note_part("endnote", [(1, r(" An endnote."))])
    comments = (f'{DECL}<w:comments {NS}><w:comment w:id="0" w:author="Ann" w:initials="A">'
                f'<w:p><w:r><w:t>A comment is not converted (issue 2275)</w:t></w:r></w:p></w:comment></w:comments>')
    fn_rels = (f'{DECL}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
               f'<Relationship Id="rIdF1" Type="{REL}/hyperlink" Target="https://example.com/fn" TargetMode="External"/>'
               "</Relationships>")

    def ref(kind, nid):
        return f'<w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><w:{kind}Reference w:id="{nid}"/></w:r>'

    body = "".join([
        p("Text with a footnote", ref("footnote", 2), " and another", ref("footnote", 1), "."),
        p("Endnote here", ref("endnote", 1), " and the first footnote again", ref("footnote", 2), "."),
        p('<w:commentRangeStart w:id="0"/>', r("commented text"), '<w:commentRangeEnd w:id="0"/>',
          '<w:r><w:commentReference w:id="0"/></w:r>'),
    ])
    write_docx("gen-notes.docx", body, parts={
        "word/footnotes.xml": footnotes, "word/endnotes.xml": endnotes, "word/comments.xml": comments,
        "word/_rels/footnotes.xml.rels": fn_rels,
    })


def mr(text, sty=None):
    rpr = f'<m:rPr><m:sty m:val="{sty}"/></m:rPr>' if sty else ""
    return f"<m:r>{rpr}<m:t>{esc(text)}</m:t></m:r>"


def math_docs():
    def frac(num, den, typ=None):
        fpr = f'<m:fPr><m:type m:val="{typ}"/></m:fPr>' if typ else "<m:fPr/>"
        return f"<m:f>{fpr}<m:num>{num}</m:num><m:den>{den}</m:den></m:f>"

    def ssup(base, sup):
        return f"<m:sSup><m:e>{base}</m:e><m:sup>{sup}</m:sup></m:sSup>"

    def ssub(base, sub):
        return f"<m:sSub><m:e>{base}</m:e><m:sub>{sub}</m:sub></m:sSub>"

    def omath(*xs):
        return f"<m:oMath>{''.join(xs)}</m:oMath>"

    def para(*xs):
        return f"<w:p><m:oMathPara>{''.join(xs)}</m:oMathPara></w:p>"

    ok = [
        p("Inline ", omath(ssup(mr("x"), mr("2")), mr("+"), ssub(mr("y"), mr("i"))), " in text."),
        para(omath(frac(mr("a"), mr("b")), mr("="), frac(mr("c"), mr("d"), "lin"), mr("="), frac(mr("e"), mr("f"), "skw"),
                   mr("="), frac(mr("g"), mr("h"), "noBar"))),
        para(omath(f"<m:rad><m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:deg/><m:e>{mr('x')}</m:e></m:rad>",
                   f"<m:rad><m:deg>{mr('3')}</m:deg><m:e>{mr('y')}</m:e></m:rad>")),
        para(omath(f"<m:nary><m:naryPr><m:chr m:val=\"∑\"/></m:naryPr><m:sub>{mr('i=1')}</m:sub>"
                   f"<m:sup>{mr('n')}</m:sup><m:e>{mr('i')}</m:e></m:nary>")),
        para(omath(f"<m:func><m:funcPr/><m:fName>{mr('sin', 'p')}</m:fName><m:e>{mr('x')}</m:e></m:func>",
                   f"<m:func><m:funcPr/><m:fName>{mr('lo', 'p')}{mr('g', 'p')}</m:fName><m:e>{mr('y')}</m:e></m:func>",
                   f"<m:func><m:funcPr/><m:fName>{mr('foo', 'p')}</m:fName><m:e>{mr('z')}</m:e></m:func>")),
        para(omath(f"<m:func><m:funcPr/><m:fName><m:limLow><m:e>{mr('lim', 'p')}</m:e>"
                   f"<m:lim>{mr('n→∞')}</m:lim></m:limLow></m:fName><m:e>{mr('a')}</m:e></m:func>")),
        para(omath(f"<m:d><m:dPr><m:begChr m:val=\"[\"/><m:endChr m:val=\"}}\"/></m:dPr><m:e>{mr('a')}</m:e><m:e>{mr('b')}</m:e></m:d>",
                   f"<m:d><m:dPr><m:begChr m:val=\"\"/></m:dPr><m:e>{mr('c')}</m:e></m:d>")),  # issue 2573
        para(omath(f"<m:m><m:mPr/><m:mr><m:e>{mr('1')}</m:e><m:e>{mr('0')}</m:e></m:mr>"
                   f"<m:mr><m:e>{mr('0')}</m:e><m:e>{mr('1')}</m:e></m:mr></m:m>")),
        para(omath(f"<m:eqArr><m:e>{mr('x=1')}</m:e><m:e>{mr('y=2')}</m:e></m:eqArr>")),
        para(omath(f"<m:acc><m:accPr><m:chr m:val=\"̃\"/></m:accPr><m:e>{mr('a')}</m:e></m:acc>",
                   f"<m:acc><m:accPr/><m:e>{mr('b')}</m:e></m:acc>",
                   f"<m:bar><m:barPr><m:pos m:val=\"bot\"/></m:barPr><m:e>{mr('c')}</m:e></m:bar>",
                   f"<m:groupChr><m:groupChrPr/><m:e>{mr('d')}</m:e></m:groupChr>")),
        para(omath(f"<m:borderBox><m:e>{mr('boxed')}</m:e></m:borderBox>", mr("+"),  # issue 2610
                   f"<m:sSubSup><m:e>{mr('x')}</m:e><m:sub>{mr('1')}</m:sub><m:sup>{mr('2')}</m:sup></m:sSubSup>",
                   f"<m:limUpp><m:e>{mr('A')}</m:e><m:lim>{mr('k')}</m:lim></m:limUpp>",
                   f"<m:sPre><m:sub>{mr('1')}</m:sub><m:sup>{mr('2')}</m:sup><m:e>{mr('C')}</m:e></m:sPre>")),
        p("Greek and symbols: ", omath(mr("\U0001d6fc+\U0001d6fd≤∞± {a_b} $5 50% #1 ~x")), "."),
        p("Equation in a table:"),
        tbl(tr(tc(p(omath(mr("E=mc"), ssup(mr(""), mr("2"))))))),
        para(omath(mr("first")), omath(mr("second"))),
        p("Text after ", omath(mr("a<b&c")), " equations."),
    ]
    write_docx("gen-math.docx", "".join(ok))
    # n-ary without m:chr (an integral) and an unsupported lower limit make
    # markitdown drop every equation in the document
    fail = [
        p("Before ", omath(ssup(mr("x"), mr("2"))), " after."),
        para(omath(f"<m:nary><m:naryPr/><m:sub>{mr('0')}</m:sub><m:sup>{mr('1')}</m:sup><m:e>{mr('f')}</m:e></m:nary>")),
        p("Text survives."),
    ]
    write_docx("gen-math-integral.docx", "".join(fail))
    fail2 = [
        p("Lower limit ", omath(f"<m:limLow><m:e>{mr('arg')}</m:e><m:lim>{mr('x')}</m:lim></m:limLow>"), " fails."),
        p("Other ", omath(mr("y")), " also gone."),
    ]
    write_docx("gen-math-limlow.docx", "".join(fail2))


def drawing(rid, descr=None, title=None, anchor=False, link=False):
    attrs = (f' descr="{esc(descr)}"' if descr is not None else "") + (f' title="{esc(title)}"' if title is not None else "")
    blip = f'r:link="{rid}"' if link else f'r:embed="{rid}"'
    tag = "wp:anchor" if anchor else "wp:inline"
    return (f'<w:r><w:drawing><{tag}><wp:extent cx="1" cy="1"/><wp:docPr id="1" name="Picture 1"{attrs}/>'
            '<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">'
            f'<pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="p.png"/><pic:cNvPicPr/></pic:nvPicPr>'
            f'<pic:blipFill><a:blip {blip}/></pic:blipFill><pic:spPr/></pic:pic></a:graphicData></a:graphic>'
            f"</{tag}></w:drawing></w:r>")


def images():
    rels = [("rIdI1", "image", "media/image1.png", False), ("rIdI2", "image", "media/photo.jpeg", False),
            ("rIdI3", "image", "https://example.com/remote.png", True), ("rIdI4", "image", "media/odd.xyz", False),
            ("rIdI5", "image", "/word/media/image1.png", False)]
    textbox = ('<w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wp:anchor><wp:docPr id="5" name="TB"/>'
               '<a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">'
               '<wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>choice text</w:t></w:r></w:p></w:txbxContent>'
               '</wps:txbx></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></mc:Choice>'
               '<mc:Fallback><w:pict><v:shape><v:textbox><w:txbxContent><w:p><w:r><w:t>fallback text box</w:t></w:r></w:p>'
               "</w:txbxContent></v:textbox></v:shape></w:pict></mc:Fallback></mc:AlternateContent></w:r>")
    body = "".join([
        p("Inline image with description: ", drawing("rIdI1", descr="A tiny\ndot")),
        p("Title only: ", drawing("rIdI2", descr="  ", title="Photo title")),
        p("No alt: ", drawing("rIdI1"), " anchored: ", drawing("rIdI1", descr="anchored", anchor=True)),
        p("Linked image is dropped: ", drawing("rIdI3", descr="remote", link=True)),
        p("Unknown extension: ", drawing("rIdI4", descr="odd")),
        p("Absolute target: ", drawing("rIdI5", descr="abs")),
        p("VML image: ", '<w:r><w:pict><v:shape><v:imagedata r:id="rIdI1" o:title="vml title"/></v:shape></w:pict></w:r>', " end"),
        p("Image in heading ", drawing("rIdI1", descr="h"), style="Heading2"),
        tbl(tr(tc(p(drawing("rIdI1", descr="cell image"))), tc(p("next to image")))),
        p("Text box content moves after the paragraph:", textbox, " paragraph end"),
        p("Image ", drawing("rIdI1", descr="with [brackets] and \"quotes\""), " alt."),
    ])
    write_docx("gen-images.docx", body, rels=rels, parts={
        "word/media/image1.png": PNG, "word/media/photo.jpeg": PNG, "word/media/odd.xyz": PNG,
    })


def robustness():
    # styles with a missing styleId (issue 1413) or w:type (issue 2166)
    styles = styles_xml(extra=(
        '<w:style w:type="paragraph"><w:name w:val="heading 1"/></w:style>'
        '<w:style w:styleId="Typeless"><w:name w:val="heading 2"/></w:style>'
        '<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="duplicate id, first wins"/></w:style>'
    ))
    body = "".join([
        p("Typeless style is a paragraph style", style="Typeless"),
        p("Heading 1 still maps", style="Heading1"),
        p("Strict-ish whitespace\n  inside   text\tnodes"),
        p("CDATA: ", r("", "", "<w:t><![CDATA[cdata <text> & more]]></w:t>")),
        p("Attribute newline in alt: ", drawing("rIdR1", descr="line&#10;break\tand tab")),
    ])
    rels = [("rIdR1", "image", "media/image1.png", False)]
    # zip entry casing differs from the central directory (issue 1812) is not
    # reproducible with zipfile; keep to package structure quirks instead
    write_docx("gen-robustness.docx", body, styles=styles, rels=rels, parts={"word/media/image1.png": PNG})
    # unmatched field end: mammoth 1.11 raised IndexError (issue 2646); 1.12.2 ignores it
    write_docx("gen-unmatched-field.docx", p("before ", '<w:r><w:fldChar w:fldCharType="end"/></w:r>', "after"))


def mammoth_1_13():
    """Reader changes in mammoth 1.12-1.13 (markitdown 0.1.9)."""
    def fld(instr, text):
        return (f'<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve">{esc(instr)}</w:instrText></w:r>'
                f'<w:r><w:fldChar w:fldCharType="separate"/></w:r>{r(text)}<w:r><w:fldChar w:fldCharType="end"/></w:r>')

    numbering = (
        f"{DECL}<w:numbering {NS}>"
        '<w:abstractNum w:abstractNumId="1"><w:numStyleLink w:val="LoopList"/></w:abstractNum>'
        '<w:abstractNum w:abstractNumId="2"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum>'
        '<w:num w:numId="7"><w:abstractNumId w:val="1"/></w:num>'
        '<w:num w:numId="8"><w:abstractNumId w:val="2"/></w:num>'
        "</w:numbering>"
    )
    styles = styles_xml(extra=(
        '<w:style w:type="numbering" w:styleId="LoopList"><w:name w:val="Loop List"/>'
        '<w:pPr><w:numPr><w:numId w:val="7"/></w:numPr></w:pPr></w:style>'
    ))
    body = "".join([
        p("Moved: ", "<w:moveFrom>" + r("old place ") + "</w:moveFrom>",
          '<w:moveToRangeStart w:id="1" w:name="move1"/>', "<w:moveTo>" + r("new place") + "</w:moveTo>",
          '<w:moveToRangeEnd w:id="1"/>'),
        "<w:moveFrom>" + p("moved-away paragraph") + "</w:moveFrom>",
        "<w:moveTo>" + p("moved-here paragraph") + "</w:moveTo>",
        p("Unquoted: ", fld(" HYPERLINK https://example.com/unquoted ", "external")),
        p("Unquoted internal: ", fld(" HYPERLINK \\l Bookmark1 ", "internal")),
        p("Quoted after two spaces: ", fld(' HYPERLINK  "https://example.com/two spaces" ', "quoted")),
        p("Switch first: ", fld(' HYPERLINK \\o "tip" "https://example.com/tip"', "not a link")),
        p("Stray separator ", '<w:r><w:fldChar w:fldCharType="separate"/></w:r>', "is ignored"),
        p("Self-linked numbering style", num=("7", "0")),
        p("Ordinary numbering", num=("8", "0")),
        "<w:customXml w:element=\"x\">" + p("custom xml paragraph") + "</w:customXml>",
    ])
    write_docx("gen-mammoth-1-13.docx", body, styles=styles, numbering=numbering)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for f in OUT.glob("gen-*.docx"):
        f.unlink()
    lists()
    headings()
    tables()
    inline()
    links()
    notes()
    math_docs()
    images()
    robustness()
    mammoth_1_13()
    for f in sorted(OUT.glob("gen-*.docx")):
        print(f.name, f.stat().st_size)


if __name__ == "__main__":
    main()
