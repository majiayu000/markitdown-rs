"""Write the generated HTML, text, CSV and notebook fixtures (gen-*).

usage: uv run --project oracle python scripts/gen_text_fixtures.py
"""

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "tests/fixtures"


def write(path, data):
    path = ROOT / path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data if isinstance(data, bytes) else data.encode("utf-8"))


MARKDOWNIFY = """<!DOCTYPE html>
<html><head><title>All the tags</title><style>p { color: red }</style>
<script>var x = "</p><b>not markup</b>";</script></head>
<body>
<h1>Heading <em>one</em></h1><h2>Two</h2><h3>Three</h3><h4>Four</h4><h5>Five</h5><h6>Six</h6><h7>Seven?</h7>
<h2>  Spaced
   heading  </h2>
<p>Plain <b>bold</b> <strong>strong</strong> <i>italic</i> <em>em</em> <u>under</u> <s>s</s> <del>del</del>
<strike>strike</strike> H<sub>2</sub>O x<sup>2</sup> <code>code `tick`</code> <kbd>Ctrl</kbd> <samp>out</samp>
<q>quoted</q> <mark>mark</mark> <span>span</span> <abbr title="t">abbr</abbr></p>
<p>Escapes: a_b *star* 1. not a list # not heading [brackets] &lt;tag&gt; &amp; &copy; &nbsp;x &#169; &#x263A; &bogus;</p>
<p>Line<br>break<br/>and<br />more</p>
<p>  leading and trailing whitespace  </p>
<p></p><p>   </p>
<pre>  preformatted
    <b>bold in pre</b> *stars* _under_
</pre>
<pre><code class="language-python">def f():
    return 1
</code></pre>
<blockquote><p>Quote para one</p><p>Quote para two</p><blockquote>Nested quote</blockquote></blockquote>
<ul><li>one</li><li>two<ul><li>two.a</li><li>two.b<ol><li>deep</li></ol></li></ul></li><li><p>para in li</p><p>second</p></li><li></li></ul>
<ol start="7"><li>seven</li><li>eight</li></ol>
<ol start="x"><li>bad start</li></ol>
<dl><dt>Term</dt><dd>Definition one</dd><dd>Definition
two</dd><dt>Term 2</dt></dl>
<table><caption>Caption</caption>
<thead><tr><th>A</th><th colspan="2">B and C</th></tr></thead>
<tbody><tr><td>1</td><td>2 | pipe</td><td><a href="/x">link</a></td></tr>
<tr><td colspan="x">bad colspan</td><td>multi<br>line</td><td><p>p in td</p></td></tr></tbody></table>
<table><tr><td>no header</td><td>row</td></tr><tr><td>second</td><td>row</td></tr></table>
<table><tr><th>only</th><th>th</th></tr><tr><td>x</td><td>y</td></tr></table>
<figure><img src="fig.png" alt="figure alt"><figcaption>Figure caption</figcaption></figure>
<hr>
<p><img src="a.png" alt="alt text" title="a &quot;title&quot;"> <img src="data:image/png;base64,iVBORw0KGgo=" alt="data">
<img data-src="real.png" src="data:image/gif;base64,R0lGOD" alt="lazy"> <img alt="no src"></p>
<p><a href="https://example.com/a b">space</a> <a href="https://example.com">https://example.com</a>
<a href="javascript:alert(1)">js</a> <a href="mailto:a@b.c">mail</a> <a href="#frag">frag</a>
<a href="https://example.com/%E4%B8%AD/中">mixed</a> <a href="rel/path?q=1#f" title="T">titled</a>
<a href="http://[bad">bad ipv6</a> <a>no href</a> <a href=""> empty href </a> <a href="https://x.y/a_b">https://x.y/a_b</a></p>
<p><input type="checkbox" checked> done <input type="checkbox"> todo <input type="text" value="x"></p>
<video src="v.mp4" poster="p.png">Video text</video> <video><source src="s.webm">Fallback</video>
<div>div text<div>nested div</div></div><section>section</section><article>article</article>
<h3><a href="/h">Link in heading</a> <img src="i.png" alt="img in heading"></h3>
<p>Nested <b>bold <i>bold italic</i> bold</b> and <b> spaced bold </b> and <b></b>empty</p>
<noscript>noscript text</noscript>
<!-- a comment -->
<p>After comment</p>
<p>Text<!-- inline comment -->split</p>
</body></html>
"""

MALFORMED = """<html><body>
<p>unclosed paragraph
<p>another <b>unclosed bold
<div>div after</b> stray end</i></div>
<table><tr><td>cell<tr><td>implicit row</table>
<ul><li>one<li>two</ul>
<p>Attributes: <a href=unquoted>u</a> <a HREF='single'>s</a> <a href="x" href="y">dup</a>
<a href = "spaced" >sp</a> <img src=a.png alt=bare/value> <img/src="weird.png"></p>
<br></br><p/>self-closing p<p>
</div></div></span>
<title>late <b>title</b> &amp; tags</title>
<textarea>  <p>not a tag</p> &amp; </textarea>
<script>if (a < b && c > d) { document.write("</div>"); }</script>
<style>a > b { }</style>
<![CDATA[cdata <text>]]> <?php echo "pi" ?> <!DOCTYPE again> <!bogus comment> <!-- unterminated -- comment --> <!---->
<p>Entities: &AMP; &amp &ampx &notin; &notit; &#0; &#128; &#x110000; &#xD800; &#65 &#x41g; &;</p>
<p>Lone &lt; and < followed by space and <3 and a < b</p>
<p>Uppercase <B>BOLD</B> <EM>EM</EM> <BR> after BR</p>
<p>Weird tags <foo:bar>ns</foo:bar> <my-element>custom</my-element> <x y="1"z="2">x</x></p>
<p>tabs\tand\r\nwindows\rlines</p>
"""

EOF_CASES = {
    "eof-tag": "<p>text</p><div class=",
    "eof-endtag": "<p>text</p></div",
    "eof-endtag-bare": "<p>text</p></",
    "eof-comment": "<p>text</p><!-- never closed",
    "eof-comment-dashes": "<p>text</p><!-- closed--",
    "eof-cdata": "<p>a</p><![CDATA[never",
    "eof-doctype": "<p>a</p><!DOCTYPE html",
    "eof-pi": "<p>a</p><?pi never",
    "eof-amp": "<p>a &am",
    "eof-amp2": "<p>a &a",
    "eof-charref": "<p>a &#123",
    "eof-charref-nosemi": "<p>a &# then text and <b>bold</b>",
    "eof-charref-semi": "<p>a &#x; then text; and <b>bold</b>",
    "eof-lt": "<p>a <",
    "eof-script": "<p>a</p><script>never closed <b>x</b>",
    "eof-title": "<title>never &amp; closed <b>x</b>",
}


# a YouTube watch page reduced to what markitdown's YouTubeConverter reads
YOUTUBE = """<!DOCTYPE html><html><head><title>Rust in 100 Seconds - YouTube</title>
<meta name="title" content="Rust in 100 Seconds">
<meta property="og:title" content="Rust in 100 Seconds (og)">
<meta name="description" content="Short description from the meta tag.">
<meta itemprop="interactionCount" content="2491634">
<meta name="keywords" content="rust, programming, tutorial">
<meta itemprop="duration" content="PT2M29S">
<meta name="empty" content="">
<meta charset="utf-8">
</head><body>
<script>var ytcfg = {};</script>
<script>var ytInitialData = {"contents":{"twoColumn":{"results":[{"videoSecondaryInfoRenderer":{"attributedDescriptionBodyText":{"content":"Rust is a memory-safe systems language.\\nLinks: https://www.rust-lang.org"}}}]}}};</script>
<script>var ytInitialData = {"attributedDescriptionBodyText":{"content":"second script is never read"}};</script>
<div id="content"><p>Fallback body text</p></div>
</body></html>
"""


def html_fixtures():
    write("html/gen-markdownify.html", MARKDOWNIFY)
    write("html/gen-malformed.html", MALFORMED)
    write("html/gen-no-body.html", "<h1>Fragment</h1><p>No html or body element, <b>just markup</b>.</p>\n<ul><li>x</li></ul>")
    write("html/gen-empty.html", "")
    write("html/gen-whitespace.html", "<html><body>  \n\t  </body></html>")
    write("html/gen-latin1.html",
          '<html><head><meta charset="iso-8859-1"></head><body><p>caf\xe9 na\xefve \xa9</p></body></html>'.encode("latin-1"))
    write("html/gen-utf8-bom.html", "﻿<html><body><p>BOM ✓</p></body></html>")
    write("html/gen-utf16.html", "<html><body><p>UTF-16 文字</p></body></html>".encode("utf-16"))
    write("html/gen-nested-lists.html", "<ul>" + "".join(f"<li>level {i}<ul>" for i in range(8)) + "<li>leaf</li>" + "</ul></li>" * 8 + "</ul>")
    write("html/gen-wikipedia-italic.html",
          '<html><head><title>Homo sapiens - Wikipedia</title></head><body>'
          '<h1 id="firstHeading" class="firstHeading"><i>Homo</i> <i>sapiens</i> (species)</h1>'
          '<div id="mw-content-text"><p><b><i>Homo sapiens</i></b> is the only extant human species.</p></div>'
          '</body></html>')
    write("html/gen-youtube.html", YOUTUBE)
    write("html/gen-youtube-multiline.html", YOUTUBE.replace("language.\\nLinks", "language.\nLinks"))
    write("html/gen-youtube-bare.html", "<html><head><title></title></head><body><p>No metadata here.</p></body></html>")
    write("html/gen-deep-divs.html", "<html><body>" + "<div>" * 120 + "deep" + "</div>" * 120 + "</body></html>")
    for name, data in EOF_CASES.items():
        write(f"html/gen-{name}.html", data)


def text_fixtures():
    write("text/gen-readme.md", "# Title  \n\nSome *markdown* with trailing spaces   \n\n\n\n\nand many blank lines\n")
    write("text/gen-crlf.txt", "line one\r\nline two  \r\n\r\n\r\n\r\nline three\rold mac\r\n")
    write("text/gen-utf8-bom.txt", "﻿BOM first line\nsecond\n")
    write("text/gen-utf16.txt", "UTF-16 text 文字\nsecond line\n".encode("utf-16"))
    write("text/gen-latin1.txt", "Caf\xe9, na\xefve, \xbd price, \xa9 2026\n".encode("latin-1"))
    write("text/gen-shift_jis.txt", "日本語のテキストです。\n二行目。\n".encode("shift_jis"))
    write("text/gen-empty.txt", b"")
    write("text/gen-data.jsonl", '{"a": 1}\n{"b": "two"}\n')
    write("text/gen-unicode.json", json.dumps({"emoji": "😀", "cjk": "中文", "rtl": "שלום"}, ensure_ascii=False, indent=2))
    write("text/gen-notebook-like.json", json.dumps({"nbformat": 4, "nbformat_minor": 5, "cells": [
        {"cell_type": "markdown", "source": ["# From a .json file\n", "notebook keys make it a notebook"]}]}))
    write("text/gen-script.py", "def main():\n    print('hello')  \n\n\nif __name__ == '__main__':\n    main()\n")
    write("text/gen-tabs.txt", "col1\tcol2\t\nvalue\t\tvalue\n\t\tindented\n")
    write("text/gen-controls.txt", "form\x0cfeed and vertical\x0btab and \x1c sep \x1d \x1e \x1f end\n")


def csv_fixtures():
    write("csv/gen-quotes.csv", 'name,quote,notes\r\n"Smith, J","He said ""hi""",plain\r\n'
          '"multi\nline",a | b,back\\slash \\| pipe\r\n,,\r\n"unterminated, quote\n')
    write("csv/gen-blank-rows.csv", "\n\n\nh1,h2\n\n\na,b\n\nc,d\n\n\n")
    write("csv/gen-ragged.csv", "a\nb,c,d\ne,f\n")
    write("csv/gen-bom.csv", "﻿col,val\nx,1\n")
    write("csv/gen-only-header.csv", "only,header\n")
    write("csv/gen-empty.csv", "")
    write("csv/gen-cr.csv", "a,b\rc,d\re,f")
    write("csv/gen-unicode.csv", "城市,人口\n東京,1400万\nМосква,1300万\n")
    write("csv/gen-latin1.csv", "name,city\nJos\xe9,M\xe1laga\n".encode("latin-1"))
    write("csv/gen-quote-midfield.csv", 'a"b,"c"d,"e" f\n')


def ipynb_fixtures():
    nb = {
        "metadata": {"title": "Notebook title", "kernelspec": {"name": "python3"}},
        "nbformat": 4, "nbformat_minor": 5,
        "cells": [
            {"cell_type": "markdown", "metadata": {}, "source": ["# Heading\n", "\n", "Some *text*  \n"]},
            {"cell_type": "code", "metadata": {}, "execution_count": 1, "outputs": [{"output_type": "stream", "text": ["ignored"]}],
             "source": "import os\nprint(os.getcwd())"},
            {"cell_type": "raw", "metadata": {}, "source": ["raw text"]},
            {"cell_type": "unknown", "source": ["skipped"]},
            {"cell_type": "markdown", "source": []},
            {"cell_type": "code", "source": ["x = 1\n", "x\n"]},
        ],
    }
    write("ipynb/gen-cells.ipynb", json.dumps(nb, indent=1))
    write("ipynb/gen-empty.ipynb", json.dumps({"nbformat": 4, "nbformat_minor": 5, "cells": []}))
    write("ipynb/gen-bad-source.ipynb", json.dumps({"cells": [{"cell_type": "code", "source": [1, 2]}]}))


RSS2 = """<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/" xml:base="https://example.com/blog/">
<channel>
  <title>
    Example
    Blog
  </title>
  <link>https://example.com/</link>
  <description>A blog about <b>things</b> &amp; stuff</description>
  <item>
    <title>First post</title>
    <link>https://example.com/blog/first</link>
    <pubDate>Mon, 05 Jan 2026 10:00:00 GMT</pubDate>
    <description>&lt;p&gt;Hello &lt;a href="second"&gt;relative link&lt;/a&gt; and &lt;img src="/img/a.png" alt="pic"&gt;&lt;/p&gt;</description>
    <content:encoded><![CDATA[<h2>Full content</h2><p>With <em>markup</em> and a <a href="../about">link</a>.</p><ul><li>one</li><li>two</li></ul>]]></content:encoded>
  </item>
  <item xml:base="https://other.example/news/">
    <title>Second post with base</title>
    <description><![CDATA[<p>See <a href="item">this</a> and <a href="">empty</a>.</p>]]></description>
  </item>
  <item>
    <description>Plain text description, no markup.</description>
  </item>
  <item>
    <title>Only a title</title>
  </item>
  <item>
    <title>Unicode — 中文 ✓</title>
    <description>&lt;p&gt;Ünïcödé &amp;amp; entities&lt;/p&gt;</description>
  </item>
</channel>
</rss>
"""

ATOM = """<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xml:base="https://example.org/feed/">
  <title type="html">Feed &lt;em&gt;title&lt;/em&gt;</title>
  <subtitle>
      Indented subtitle
      second line
  </subtitle>
  <updated>2026-02-01T00:00:00Z</updated>
  <entry>
    <title>Plain summary</title>
    <updated>2026-02-02T00:00:00Z</updated>
    <summary>First line
        indented continuation
          deeper
        back</summary>
  </entry>
  <entry xml:base="entries/">
    <title type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml">XHTML <b>title</b></div></title>
    <summary type="html">&lt;p&gt;HTML &lt;a href="one"&gt;summary&lt;/a&gt;&lt;/p&gt;</summary>
    <content type="xhtml">
      <div xmlns="http://www.w3.org/1999/xhtml" xml:base="nested/">
        <p>XHTML content with <a href="link">a link</a> &amp; <img src="i.png" alt="img"/>.</p>
        <br/>
      </div>
    </content>
  </entry>
  <entry>
    <title>Media types</title>
    <content type="text/html">&lt;h3&gt;text/html content&lt;/h3&gt;</content>
  </entry>
  <entry>
    <title>Binary content is dropped</title>
    <summary>But the summary stays.</summary>
    <content type="image/png">iVBORw0KGgo=</content>
  </entry>
  <entry>
    <title>Other text type</title>
    <content type="text/plain">   plain *text* content   </content>
  </entry>
  <entry>
    <title type="text">  </title>
    <content type="application/atom+xml"><x xmlns="urn:x">inline xml <y>node</y></x></content>
  </entry>
</feed>
"""

ATOM_PREFIXED = """<?xml version="1.0"?>
<a:feed xmlns:a="http://www.w3.org/2005/Atom" xmlns:other="urn:other">
  <a:title>Prefixed feed</a:title>
  <other:title>ignored, other namespace</other:title>
  <a:entry><a:title>Entry</a:title><a:summary type="html">&lt;b&gt;bold&lt;/b&gt;</a:summary></a:entry>
</a:feed>
"""


def rss_fixtures():
    write("rss/gen-rss2.xml", RSS2)
    write("rss/gen-feed.rss", RSS2.replace("Example\n    Blog", "RSS extension"))
    write("rss/gen-atom.atom", ATOM)
    write("rss/gen-atom-prefixed.xml", ATOM_PREFIXED)
    write("rss/gen-atom-no-ns.xml", "<feed><title>No namespace</title><entry><title>E</title><content>c</content></entry></feed>")
    write("rss/gen-latin1.xml", '<?xml version="1.0" encoding="ISO-8859-1"?>\n<rss><channel><title>Caf\xe9</title>'
          '<item><title>na\xefve</title><description>&lt;p&gt;\xbd&lt;/p&gt;</description></item></channel></rss>'.encode("latin-1"))
    write("rss/gen-entity.xml", '<?xml version="1.0"?>\n<!DOCTYPE rss [<!ENTITY x "expanded">]>\n<rss><channel><title>&x;</title></channel></rss>\n')
    write("rss/gen-not-a-feed.xml", '<?xml version="1.0"?>\n<catalog>\n  <book id="1">  <title>Not a feed</title>  </book>\n</catalog>\n')
    write("rss/gen-feed-no-entries.xml", '<feed xmlns="http://www.w3.org/2005/Atom"><title>No entries</title></feed>')
    write("rss/gen-rss-no-channel.rss", "<rss><item><title>no channel</title></item></rss>")
    write("rss/gen-broken.xml", "<rss><channel><title>broken</title></channel>")


def zip_bytes(files):
    import io
    import zipfile
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in files:
            z.writestr(zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0)), data)
    return buf.getvalue()


def epub(opf_path, opf, files):
    container = ('<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
                 f'<rootfiles><rootfile full-path="{opf_path}" media-type="application/oebps-package+xml"/></rootfiles></container>')
    return zip_bytes([("mimetype", "application/epub+zip"), ("META-INF/container.xml", container), (opf_path, opf)] + files)


def epub_fixtures():
    opf = """<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>  A <![CDATA[Generated]]> Book  </dc:title>
    <dc:creator>First Author</dc:creator><dc:creator> </dc:creator><dc:creator>Second <i>Author</i></dc:creator>
    <dc:language>en</dc:language>
    <dc:identifier id="id">urn:uuid:1234</dc:identifier>
    <dc:date>2026-10-22</dc:date>
  </metadata>
  <manifest>
    <item id="c1" href="text/chapter%201.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="text/ch2.html" media-type="text/html"/>
    <item id="c3" href="../outside.xhtml" media-type="application/xhtml+xml"/>
    <item id="img" href="images/a.png" media-type="image/png"/>
    <item id="dup" href="first.xhtml"/><item id="dup" href="text/second.xhtml"/>
  </manifest>
  <spine><itemref idref="c1"/><itemref idref="missing"/><itemref idref="c2"/><itemref idref="dup"/><itemref idref="c3"/></spine>
</package>"""
    files = [
        ("OEBPS/text/chapter 1.xhtml", '<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>One</title></head>'
         '<body><h1>Chapter 1</h1><p>First <em>chapter</em> text with a <a href="ch2.html">link</a>.</p></body></html>'),
        ("OEBPS/text/ch2.html", "<html><body><h2>Chapter 2</h2><ul><li>a</li><li>b</li></ul></body></html>"),
        ("OEBPS/text/second.xhtml", "<html><body><p>Second definition wins.</p></body></html>"),
        ("outside.xhtml", "<html><body><p>Outside the OPF directory.</p></body></html>"),
        ("OEBPS/images/a.png", b"\x89PNG\r\n\x1a\n"),
    ]
    write("epub/gen-book.epub", epub("OEBPS/content.opf", opf, files))
    bare = """<package xmlns:opf="http://www.idpf.org/2007/opf"><opf:manifest><opf:item id="x" href="x.html"/></opf:manifest>
<opf:spine><opf:itemref idref="x"/></opf:spine></package>"""
    write("epub/gen-prefixed-opf.epub", epub("content.opf", bare, [("x.html", "<p>never read: prefixed OPF</p>")]))
    write("epub/gen-no-metadata.epub", epub("book.opf", '<package><manifest><item id="a" href="a.html"/></manifest>'
                                        '<spine><itemref idref="a"/></spine></package>', [("a.html", "<h1>Only content</h1>")]))


def zip_fixtures():
    root = ROOT
    inner = zip_bytes([("inner/readme.txt", "inside a nested zip\n"), ("inner/data.csv", "a,b\n1,2\n")])
    files = [
        ("docs/", b""),
        ("docs/report.docx", (root / "docx/mammoth-simple-list.docx").read_bytes()),
        ("docs/page.HTML", "<html><body><h1>Upper-case extension</h1></body></html>"),
        ("data/table.csv", "x,y\n1,2\n"),
        ("notes.md", "# Notes\n\ntrailing   \n"),
        ("nested.zip", inner),
        ("binary.bin", bytes(range(256)) * 4),
        ("empty.txt", b""),
        ("unicode/日本語.txt", "テキスト\n"),
        ("slides.pptx", (root / "pptx/python-pptx-minimal.pptx").read_bytes()),
        ("feed.xml", RSS2),
    ]
    write("zip/gen-mixed.zip", zip_bytes(files))
    write("zip/gen-empty.zip", zip_bytes([]))


if __name__ == "__main__":
    for d in ("html", "text", "csv", "ipynb", "rss", "epub", "zip"):
        for f in (ROOT / d).glob("gen-*"):
            f.unlink()
    html_fixtures()
    text_fixtures()
    csv_fixtures()
    ipynb_fixtures()
    rss_fixtures()
    epub_fixtures()
    zip_fixtures()
    print("ok")
