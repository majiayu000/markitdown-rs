"""MarkItDown's constructor options, converter registry and plugins.

These tests only use markitdown's public API, so they also run against
Python markitdown itself (except the ones marked `rs_only`):

    uv run --project oracle --with pytest pytest crates/markitdown-py/tests/test_api.py
"""

import io
import sys
import textwrap
import warnings
from pathlib import Path

import pytest

import markitdown
from markitdown import (
    PRIORITY_GENERIC_FILE_FORMAT,
    PRIORITY_SPECIFIC_FILE_FORMAT,
    DocumentConverter,
    DocumentConverterResult,
    FileConversionException,
    MarkItDown,
    StreamInfo,
    UnsupportedFormatException,
)
from markitdown.converters import DocxConverter, HtmlConverter

ROOT = Path(__file__).resolve().parents[3]
FIXTURES = ROOT / "tests/fixtures"
IS_RS = hasattr(markitdown, "_markitdown_rs")
rs_only = pytest.mark.skipif(not IS_RS, reason="markitdown-rs behaviour")


def docx_bytes():
    return (FIXTURES / "docx/markitdown-test.docx").read_bytes()


class Shouting(DocumentConverter):
    """Accepts .shout files and upper-cases them."""

    def accepts(self, file_stream, stream_info, **kwargs):
        return (stream_info.extension or "") == ".shout"

    def convert(self, file_stream, stream_info, **kwargs):
        text = file_stream.read().decode()
        return DocumentConverterResult(markdown=text.upper() + "   \n\n\n\nend", title="T")


class Failing(DocumentConverter):
    def accepts(self, file_stream, stream_info, **kwargs):
        return True

    def convert(self, file_stream, stream_info, **kwargs):
        raise ValueError("nope")


def test_priorities():
    assert PRIORITY_SPECIFIC_FILE_FORMAT == 0.0
    assert PRIORITY_GENERIC_FILE_FORMAT == 10.0


@rs_only
def test_unknown_constructor_arguments_raise():
    with pytest.raises(TypeError, match="llm_modle"):
        MarkItDown(llm_modle="gpt-4o")
    # plugins may read keyword arguments of their own
    MarkItDown(enable_plugins=True, ocr_language="de")


def test_custom_converter_and_normalized_output():
    md = MarkItDown()
    md.register_converter(Shouting())
    r = md.convert_stream(io.BytesIO(b"hi there"), stream_info=StreamInfo(extension=".shout"))
    # markitdown strips trailing spaces and squeezes blank lines
    assert r.markdown == "HI THERE\n\nend"
    assert r.title == "T"


def test_without_builtins_nothing_converts_until_registered():
    md = MarkItDown(enable_builtins=False)
    with pytest.raises(UnsupportedFormatException):
        md.convert_stream(io.BytesIO(docx_bytes()), stream_info=StreamInfo(extension=".docx"))
    md.register_converter(Shouting())
    assert md.convert_stream(io.BytesIO(b"a"), stream_info=StreamInfo(extension=".shout")).markdown == "A\n\nend"
    md.enable_builtins()
    assert md.convert_stream(io.BytesIO(docx_bytes()), stream_info=StreamInfo(extension=".docx")).markdown.startswith("\n") is False


def test_failed_converter_falls_through_and_is_reported():
    md = MarkItDown()
    md.register_converter(Failing())
    want = MarkItDown().convert_stream(io.BytesIO(docx_bytes()), stream_info=StreamInfo(extension=".docx")).markdown
    got = md.convert_stream(io.BytesIO(docx_bytes()), stream_info=StreamInfo(extension=".docx")).markdown
    assert got == want
    only_failing = MarkItDown(enable_builtins=False)
    only_failing.register_converter(Failing())
    with pytest.raises(FileConversionException) as e:
        only_failing.convert_stream(io.BytesIO(b"x"))
    assert "Failing threw ValueError with message: nope" in str(e.value)


def test_priority_orders_custom_converters_around_builtins():
    class TxtToo(Shouting):
        def accepts(self, file_stream, stream_info, **kwargs):
            return (stream_info.extension or "") == ".txt"

    generic = MarkItDown()
    generic.register_converter(TxtToo(), priority=PRIORITY_GENERIC_FILE_FORMAT + 1)
    stream = lambda: io.BytesIO(b"quiet")
    info = StreamInfo(extension=".txt")
    assert generic.convert_stream(stream(), stream_info=info).markdown == "quiet"
    first = MarkItDown()
    first.register_converter(TxtToo(), priority=PRIORITY_GENERIC_FILE_FORMAT)
    assert first.convert_stream(stream(), stream_info=info).markdown == "QUIET\n\nend"


def test_register_page_converter_is_deprecated():
    md = MarkItDown(enable_builtins=False)
    with pytest.warns(DeprecationWarning):
        md.register_page_converter(Shouting())
    assert md.convert_stream(io.BytesIO(b"a"), stream_info=StreamInfo(extension=".shout")).markdown == "A\n\nend"


def test_builtin_converters_are_subclassable():
    class Tagged(DocxConverter):
        def convert(self, file_stream, stream_info, **kwargs):
            r = super().convert(file_stream, stream_info, **kwargs)
            return DocumentConverterResult(markdown="TAG\n" + r.markdown)

    md = MarkItDown()
    md.register_converter(Tagged())
    plain = MarkItDown().convert_stream(io.BytesIO(docx_bytes()), stream_info=StreamInfo(extension=".docx"))
    got = md.convert_stream(io.BytesIO(docx_bytes()), stream_info=StreamInfo(extension=".docx"))
    assert got.markdown == "TAG\n" + plain.markdown
    assert HtmlConverter().convert_string("<h1>Hi</h1><p>x</p>").markdown == "# Hi\n\nx"


class FakeResponse:
    def __init__(self, data, headers, url):
        self._data, self.headers, self.url = data, headers, url

    def raise_for_status(self):
        pass

    def iter_content(self, chunk_size):
        yield self._data


class FakeSession:
    def __init__(self):
        self.calls = []

    def get(self, url, stream=False):
        self.calls.append(url)
        return FakeResponse(b"<h1>From session</h1>", {"content-type": "text/html"}, url)


def test_requests_session_is_used_for_http():
    session = FakeSession()
    md = MarkItDown(requests_session=session)
    # nothing listens there: only the session can answer
    r = md.convert("http://127.0.0.1:9/page")
    assert session.calls == ["http://127.0.0.1:9/page"]
    assert r.markdown == "# From session"


def stub_exiftool(tmp_path):
    stub = tmp_path / "exiftool"
    stub.write_text((ROOT / "tests/exiftool-stub.sh").read_text())
    stub.chmod(0o755)
    return str(stub)


@pytest.mark.skipif(sys.platform == "win32", reason="shell script stub")
def test_exiftool_path(tmp_path):
    png = FIXTURES / "media/gen-red.png"
    want = (ROOT / "tests/snapshots/media-exiftool/gen-red.png.md").read_text()[:-1]  # the CLI adds a newline
    assert MarkItDown(exiftool_path=stub_exiftool(tmp_path)).convert(str(png)).markdown == want
    with pytest.raises(FileConversionException, match="exiftool"):
        MarkItDown(exiftool_path=str(tmp_path / "missing")).convert(str(png))
    assert MarkItDown(exiftool_path="").convert(str(png)).markdown == ""


class FakeLLM:
    """An OpenAI-style client: client.chat.completions.create(...)."""

    def __init__(self, answer="  A red square.  "):
        self.requests = []
        self.answer = answer
        self.chat = self
        self.completions = self

    def create(self, model, messages):
        self.requests.append((model, messages))
        message = type("Message", (), {"content": self.answer})
        choice = type("Choice", (), {"message": message})
        return type("Response", (), {"choices": [choice]})


def test_llm_image_caption():
    llm = FakeLLM()
    md = MarkItDown(llm_client=llm, llm_model="gpt-test", exiftool_path="")
    r = md.convert(str(FIXTURES / "media/gen-red.png"))
    assert r.markdown == "\n# Description:\nA red square.\n"
    model, messages = llm.requests[0]
    assert model == "gpt-test"
    content = messages[0]["content"]
    assert content[0] == {"type": "text", "text": "Write a detailed caption for this image."}
    assert content[1]["image_url"]["url"].startswith("data:image/png;base64,iVBOR")
    # a custom prompt, per instance or per call
    MarkItDown(llm_client=llm, llm_model="m", llm_prompt="Name the colour.", exiftool_path="").convert(
        str(FIXTURES / "media/gen-red.png")
    )
    assert llm.requests[1][1][0]["content"][0]["text"] == "Name the colour."


def test_llm_needs_a_model():
    llm = FakeLLM()
    r = MarkItDown(llm_client=llm, exiftool_path="").convert(str(FIXTURES / "media/gen-red.png"))
    assert r.markdown == "" and llm.requests == []


def test_audio_transcription(monkeypatch):
    calls = []

    def fake_transcribe(file_stream, *, audio_format, language):
        calls.append((audio_format, language, len(file_stream.read())))
        return "hello world"

    target = sys.modules[markitdown.converters.AudioConverter.__module__]
    monkeypatch.setattr(target, "transcribe_audio", fake_transcribe)
    md = MarkItDown(exiftool_path="")
    r = md.convert(str(FIXTURES / "media/markitdown-test.wav"))
    assert r.markdown == "### Audio Transcript:\nhello world"
    assert calls[0][:2] == ("wav", "en-US")
    md.convert(str(FIXTURES / "media/markitdown-test.mp3"), language="de-DE")
    assert calls[1][:2] == ("mp3", "de-DE")


def test_youtube_transcript(monkeypatch):
    class Part:
        def __init__(self, text):
            self.text = text

    class Track:
        language_code = "en"

    class FakeApi:
        def list(self, video_id):
            assert video_id == "5C_HPTJg5ek"
            return [Track()]

        def fetch(self, video_id, languages):
            assert languages == ["en", "en"]
            return [Part("never gonna"), Part("give you up")]

    target = sys.modules[markitdown.converters.YouTubeConverter.__module__]
    monkeypatch.setattr(target, "IS_YOUTUBE_TRANSCRIPT_CAPABLE", True)
    monkeypatch.setattr(target, "YouTubeTranscriptApi", FakeApi, raising=False)
    html = (FIXTURES / "html/gen-youtube.html").read_bytes()
    url = "https://www.youtube.com/watch?v=5C_HPTJg5ek"
    want = (ROOT / "tests/url/youtube.md").read_text().rstrip("\n")
    r = MarkItDown().convert_stream(io.BytesIO(html), stream_info=StreamInfo(extension=".html", url=url))
    assert r.markdown == want + "\n\n### Transcript\nnever gonna give you up\n"


def test_plugins_from_entry_points(tmp_path, monkeypatch):
    dist = tmp_path / "shout_plugin-1.0.dist-info"
    dist.mkdir()
    (dist / "METADATA").write_text("Metadata-Version: 2.1\nName: shout-plugin\nVersion: 1.0\n")
    (dist / "entry_points.txt").write_text("[markitdown.plugin]\nshout = shout_plugin\n")
    (tmp_path / "shout_plugin.py").write_text(
        textwrap.dedent(
            """
            from markitdown import DocumentConverter, DocumentConverterResult

            __plugin_interface_version__ = 1
            seen = {}

            class Shout(DocumentConverter):
                def accepts(self, file_stream, stream_info, **kwargs):
                    return stream_info.extension == ".shout"

                def convert(self, file_stream, stream_info, **kwargs):
                    return DocumentConverterResult(markdown=file_stream.read().decode().upper())

            def register_converters(markitdown, **kwargs):
                seen.update(kwargs)
                markitdown.register_converter(Shout())
            """
        )
    )
    monkeypatch.syspath_prepend(str(tmp_path))
    monkeypatch.setattr(sys.modules[MarkItDown.__module__], "_plugins", None)
    md = MarkItDown(enable_plugins=True, shout_volume=11)
    assert md.convert_stream(io.BytesIO(b"hey"), stream_info=StreamInfo(extension=".shout")).markdown == "HEY"
    assert sys.modules["shout_plugin"].seen == {"shout_volume": 11}
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        with pytest.raises(RuntimeWarning):
            md.enable_plugins()


def test_document_intelligence_with_a_mocked_client(monkeypatch):
    pytest.importorskip("azure.ai.documentintelligence")
    from markitdown.converters import _doc_intel_converter as di

    seen = {}

    class Poller:
        def result(self):
            return type("Result", (), {"content": "# Analyzed\n\n<!-- PageBreak -->\nbody"})

    class FakeClient:
        def __init__(self, endpoint, credential, **kwargs):
            seen.update(endpoint=endpoint, **kwargs)

        def begin_analyze_document(self, **kwargs):
            seen["features"] = kwargs.get("features")
            return Poller()

    monkeypatch.setattr(di, "DocumentIntelligenceClient", FakeClient)
    md = MarkItDown(
        docintel_endpoint="https://example.cognitiveservices.azure.com/",
        docintel_credential=di.AzureKeyCredential("not-a-real-key"),
    )
    r = md.convert(str(FIXTURES / "docx/markitdown-test.docx"))
    assert seen["endpoint"] == "https://example.cognitiveservices.azure.com/"
    assert r.markdown == "# Analyzed\n\nbody"


def test_document_intelligence_needs_its_sdk():
    try:
        import azure.ai.documentintelligence  # noqa: F401
    except ImportError:
        with pytest.raises(markitdown.MissingDependencyException):
            MarkItDown(docintel_endpoint="https://example.cognitiveservices.azure.com/")
    else:
        pytest.skip("the Azure SDK is installed")


def test_style_map():
    pytest.importorskip("mammoth")
    docx = str(FIXTURES / "docx/markitdown-test.docx")
    plain = MarkItDown().convert(docx).markdown
    r = MarkItDown(style_map="p[style-name='Heading 1'] => h2:fresh").convert(docx).markdown
    assert "\n# Abstract\n" in "\n" + plain and "\n## Abstract\n" in "\n" + r
    # per call, and a run style: bold becomes underline
    inline = str(FIXTURES / "docx/gen-inline.docx")
    assert "**" in MarkItDown().convert(inline).markdown
    r = MarkItDown().convert(inline, style_map="b => u").markdown
    assert r.startswith("<u>bold</u> *italic*")


@rs_only
def test_style_map_needs_mammoth(monkeypatch):
    monkeypatch.setitem(sys.modules, "mammoth", None)
    md = MarkItDown(enable_builtins=False)
    md.register_converter(markitdown.converters.DocxConverter())
    with pytest.raises(FileConversionException, match=r"markitdown-rs\[docx\]"):
        md.convert(str(FIXTURES / "docx/markitdown-test.docx"), style_map="b => u")


def test_content_understanding_with_a_mocked_client(monkeypatch):
    pytest.importorskip("azure.ai.contentunderstanding")
    from markitdown.converters import _cu_converter as cu

    seen = {}

    class FakeClient:
        def __init__(self, endpoint, credential, **kwargs):
            seen["endpoint"] = endpoint

        def begin_analyze_binary(self, analyzer_id, binary_input, content_type):
            seen.update(analyzer_id=analyzer_id, content_type=content_type, size=len(binary_input))
            return type("Poller", (), {"result": lambda self: "analysis"})()

    monkeypatch.setattr(cu, "ContentUnderstandingClient", FakeClient)
    monkeypatch.setattr(cu, "to_llm_input", lambda result: f"---\nsource: {result}\n---\n\n# Analyzed")
    md = MarkItDown(
        cu_endpoint="https://example.services.ai.azure.com/",
        cu_credential=cu.AzureKeyCredential("not-a-real-key"),
    )
    pdf = FIXTURES / "pdf/markitdown-test.pdf"
    r = md.convert(str(pdf))
    assert seen["endpoint"] == "https://example.services.ai.azure.com/"
    assert seen["content_type"] == "application/pdf"
    assert seen["size"] == pdf.stat().st_size
    assert r.markdown == "---\nsource: analysis\n---\n\n# Analyzed"


def test_pptx_pictures_get_llm_captions():
    llm = FakeLLM("A chart.\nWith [brackets]")
    pptx = str(FIXTURES / "pptx/markitdown-test.pptx")
    plain = MarkItDown().convert(pptx).markdown
    r = MarkItDown(llm_client=llm, llm_model="m").convert(pptx).markdown
    assert len(llm.requests) == 2
    # the caption leads the alt text; newlines and brackets become spaces
    assert "![A chart. With brackets This phrase of the caption is Human-written.](Picture4.jpg)" in r
    assert r.replace("A chart. With brackets ", "") == plain


class Ocr:
    """An `_image_to_html` override that numbers the images it sees."""

    def __init__(self, fragment):
        self.fragment = fragment
        self.seen = []

    def __call__(self, converter, image_stream, stream_info, **kwargs):
        self.seen.append((stream_info.mimetype, stream_info.extension, len(image_stream.read())))
        return self.fragment.format(n=len(self.seen))


def hooked(base, ocr):
    return type("Hooked" + base.__name__, (base,), {"_image_to_html": lambda self, s, i, **k: ocr(self, s, i, **k)})


def test_docx_image_hook_splits_paragraphs():
    from markitdown.converters import DocxConverter

    ocr = Ocr("<p><em>[Image {n}]<br>text &amp; more</em></p>")
    md = MarkItDown()
    md.register_converter(hooked(DocxConverter, ocr)())
    r = md.convert(str(FIXTURES / "docx/gen-images.docx")).markdown
    assert len(ocr.seen) == 10
    assert ocr.seen[1][:2] == ("image/jpeg", ".jpg")
    assert ocr.seen[4][:2] == (None, None)  # an unknown content type
    # the fragment's paragraph leaves the image's paragraph, heading and
    # emphasis, but not its table cell
    assert r.startswith("Inline image with description:\n\n*[Image 1]\ntext & more*\n\nTitle only:")
    assert "## Image in heading\n\n*[Image 8]\ntext & more*\n\n" in r
    assert "| *[Image 9] text & more* | next to image |" in r
    assert "data:image" not in r


def test_pptx_and_xlsx_image_hooks():
    from markitdown.converters import PptxConverter, XlsxConverter

    md = MarkItDown()
    ocr = Ocr("<b>pic {n}</b>")
    md.register_converter(hooked(PptxConverter, ocr)())
    r = md.convert(str(FIXTURES / "pptx/gen-groups-pictures.pptx")).markdown
    assert "**pic 1**" in r and "![" not in r
    xocr = Ocr("<p>img {n}</p>")
    md.register_converter(hooked(XlsxConverter, xocr)())
    r = md.convert(str(Path(__file__).parent / "data/images.xlsx")).markdown
    assert r == (
        "## Data\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n### Images in this sheet:\n\nimg 1\n\nimg 2\n\n"
        "## Empty\n|\n|  |\n\n## More\n| x |\n| --- |\n\n### Images in this sheet:\n\nimg 3"
    )
    assert [s[:2] for s in xocr.seen] == [("image/png", ".png")] * 3


def test_failing_image_hook_fails_the_conversion():
    from markitdown.converters import DocxConverter

    def boom(self, image_stream, stream_info, **kwargs):
        raise RuntimeError("vision service down")

    md = MarkItDown(enable_builtins=False)
    md.register_converter(type("Boom", (DocxConverter,), {"_image_to_html": boom})())
    with pytest.raises(FileConversionException, match="vision service down"):
        md.convert(str(FIXTURES / "docx/gen-images.docx"))


def test_markitdown_ocr_plugin(monkeypatch):
    pytest.importorskip("markitdown_ocr")
    monkeypatch.setattr(sys.modules[MarkItDown.__module__], "_plugins", None)
    llm = FakeLLM("Scanned words")
    md = MarkItDown(enable_plugins=True, llm_client=llm, llm_model="m")
    r = md.convert(str(FIXTURES / "docx/gen-images.docx")).markdown
    assert r.count("*[Image OCR]\nScanned words\n[End OCR]*") == 9
    # in presentations the LLM caption comes first and becomes the alt text
    r = md.convert(str(FIXTURES / "pptx/gen-groups-pictures.pptx")).markdown
    assert "![Scanned words image.png](PicturePlaceholder2.jpg)" in r


@pytest.mark.parametrize(
    "fixture,url,title",
    [
        ("html/markitdown-test_blog.html", None, "Does Model and Inference Parameter Matter in LLM Applications? - A Case Study for MATH | AutoGen"),
        ("html/gen-youtube.html", "https://www.youtube.com/watch?v=5C_HPTJg5ek", "Rust in 100 Seconds"),
        ("html/gen-youtube.html", "https://www.youtube.com/feed/trending", "Rust in 100 Seconds - YouTube"),
        ("html/markitdown-test_wikipedia.html", "https://en.wikipedia.org/wiki/Microsoft", "Microsoft"),
        ("html/gen-wikipedia-italic.html", "https://en.wikipedia.org/wiki/Homo_sapiens", "Homo sapiens (species)"),
        ("html/markitdown-test_serp.html", "https://www.bing.com/search?q=microsoft+wikipedia", "Microsoft wikipedia - Search"),
        ("rss/markitdown-test_rss.xml", None, "The Official Microsoft Blog"),
        ("rss/gen-atom.atom", None, "Feed title"),
        ("epub/markitdown-test.epub", None, "Test EPUB Document"),
        ("ipynb/markitdown-test_notebook.ipynb", None, "Test Notebook Title"),
        ("ipynb/gen-cells.ipynb", None, "Notebook title"),
        ("msg/markitdown-test_outlook_msg.msg", None, "Test Email Message"),
        ("html/gen-empty.html", None, None),
        ("docx/markitdown-test.docx", None, None),
    ],
)
def test_titles(fixture, url, title):
    data = (FIXTURES / fixture).read_bytes()
    info = StreamInfo(extension="." + fixture.rsplit(".", 1)[1], url=url)
    assert MarkItDown().convert_stream(io.BytesIO(data), stream_info=info).title == title


def run_cli(*args, stdin=None):
    import subprocess

    return subprocess.run(
        [sys.executable, "-m", "markitdown", *args],
        input=stdin,
        capture_output=True,
        env={**__import__("os").environ, "EXIFTOOL_PATH": ""},
        cwd=ROOT,
    )


def test_python_cli():
    snapshot = (ROOT / "tests/snapshots/docx/markitdown-test.docx.md").read_bytes()
    r = run_cli("tests/fixtures/docx/markitdown-test.docx")
    assert r.returncode == 0 and r.stdout == snapshot
    r = run_cli("-x", "docx", stdin=(FIXTURES / "docx/markitdown-test.docx").read_bytes())
    assert r.stdout == snapshot
    r = run_cli("--list-plugins")
    assert r.returncode == 0 and b"Installed MarkItDown 3rd-party Plugins:" in r.stdout
    r = run_cli("-m", "bogus", stdin=b"x")
    assert r.returncode == 1 and r.stdout == b"Invalid MIME type: bogus\n"
    r = run_cli("-d", "tests/fixtures/docx/markitdown-test.docx")
    assert r.returncode == 1 and b"Document Intelligence Endpoint is required" in r.stdout


@rs_only
def test_python_cli_version():
    r = run_cli("--version")
    assert r.stdout.decode().strip() == f"markitdown {markitdown.__version__}"


@rs_only
def test_guard_detects_microsofts_markitdown(tmp_path, monkeypatch, capsys):
    import importlib.util

    spec = importlib.util.spec_from_file_location(
        "markitdown_rs_guard", Path(__file__).parents[1] / "markitdown_rs_guard.py"
    )
    guard = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(guard)  # nothing to report next to the tests
    (tmp_path / "markitdown_rs-0.2.0.dist-info").mkdir()
    assert guard.check(str(tmp_path)) is None
    (tmp_path / "markitdown_ocr-0.1.2.dist-info").mkdir()
    assert guard.check(str(tmp_path)) is None
    (tmp_path / "markitdown-0.1.9.dist-info").mkdir()
    assert guard.check(str(tmp_path)) == "markitdown-0.1.9.dist-info"
    monkeypatch.setattr(guard, "__file__", str(tmp_path / "markitdown_rs_guard.py"))
    guard.warn()
    assert "pip install --no-deps markitdown-ocr" in capsys.readouterr().err
    monkeypatch.setenv("MARKITDOWN_RS_ALLOW_MARKITDOWN", "1")
    guard.warn()
    assert capsys.readouterr().err == ""
