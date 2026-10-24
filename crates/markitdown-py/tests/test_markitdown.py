"""The `markitdown` module against the markitdown CLI and Python
markitdown's snapshots. Run from the repository root (see README):

    uv run --project crates/markitdown-py pytest crates/markitdown-py/tests
"""

import io
import subprocess
from pathlib import Path

import pytest

from markitdown import (
    DocumentConverterResult,
    FileConversionException,
    MarkItDown,
    StreamInfo,
    UnsupportedFormatException,
)

ROOT = Path(__file__).resolve().parents[3]
CLI = ROOT / "target/release/markitdown"
# one or more fixtures per converter; paths as gen_snapshots.sh passes them
FIXTURES = [
    "docx/markitdown-test.docx",
    "docx/gen-mammoth-1-13.docx",
    "pptx/markitdown-test.pptx",
    "pdf/markitdown-test.pdf",
    "pdf/gen-form-mixed.pdf",
    "pdf/gen-encrypt-aes-256.pdf",
    "pdf/gen-cjk.pdf",
    "xlsx/markitdown-test.xlsx",
    "xls/markitdown-test.xls",
    "html/markitdown-test_blog.html",
    "epub/markitdown-test.epub",
    "msg/markitdown-test_outlook_msg.msg",
    "csv/markitdown-test_mskanji.csv",
    "ipynb/markitdown-test_notebook.ipynb",
    "rss/markitdown-test_rss.xml",
    "zip/markitdown-test_files.zip",
    "text/markitdown-test.json",
]


@pytest.fixture(scope="module")
def md():
    return MarkItDown()


@pytest.fixture(autouse=True)
def at_root(monkeypatch):
    monkeypatch.chdir(ROOT)


def cli(arg: str) -> bytes:
    return subprocess.run([str(CLI), arg], check=True, capture_output=True).stdout


@pytest.mark.parametrize("name", FIXTURES)
def test_same_bytes_as_cli_and_python(md, name):
    path = f"tests/fixtures/{name}"
    result = md.convert(path)
    assert isinstance(result, DocumentConverterResult)
    got = (result.markdown + "\n").encode()
    assert got == cli(path)
    assert got == (ROOT / f"tests/snapshots/{name}.md").read_bytes()
    assert result.text_content == result.markdown == str(result)


def test_stream_with_hint_and_path_object(md):
    data = (ROOT / "tests/fixtures/docx/markitdown-test.docx").read_bytes()
    want = cli("tests/fixtures/docx/markitdown-test.docx").decode()
    got = md.convert_stream(io.BytesIO(data), stream_info=StreamInfo(extension=".docx"))
    assert got.markdown + "\n" == want
    assert md.convert(io.BytesIO(data)).markdown + "\n" == want  # sniffed
    assert md.convert(ROOT / "tests/fixtures/docx/markitdown-test.docx").markdown + "\n" == want


def test_uris(md):
    assert md.convert("data:text/html;base64,PGgxPkhpPC9oMT4=").markdown == "# Hi"
    uri = (ROOT / "tests/fixtures/pptx/markitdown-test.pptx").as_uri()
    assert md.convert(uri).markdown + "\n" == cli(uri).decode()


def test_errors(md):
    with pytest.raises(FileNotFoundError):
        md.convert("tests/fixtures/nope.docx")
    with pytest.raises(UnsupportedFormatException):
        md.convert_stream(io.BytesIO(bytes(range(256)) * 4), stream_info=StreamInfo(extension=".bin"))
    with pytest.raises(FileConversionException):
        md.convert_stream(io.BytesIO(b"PK\x03\x04" + bytes(range(256)) * 2), stream_info=StreamInfo(extension=".docx"))
    # text that merely starts like a zip falls back to plain text, as in markitdown
    text = md.convert_stream(io.BytesIO(b"PK\x03\x04 not really a zip"), stream_info=StreamInfo(extension=".docx"))
    assert text.markdown == "PK\x03\x04 not really a zip"
    with pytest.raises(ValueError, match="Unsupported URI scheme"):
        md.convert_uri("ftp://example.com/a.pdf")
    with pytest.raises(TypeError):
        md.convert(42)


class FakeResponse:
    """What markitdown reads from a requests.Response."""

    def __init__(self, data, headers, url):
        self._data, self.headers, self.url = data, headers, url

    def iter_content(self, chunk_size):
        yield self._data


def test_response_named_by_content_disposition(md):
    data = (ROOT / "tests/fixtures/docx/markitdown-test.docx").read_bytes()
    resp = FakeResponse(
        data,
        {"content-type": "application/octet-stream", "content-disposition": "attachment; filename*=UTF-8''r%C3%A9sum%C3%A9.docx"},
        "https://example.com/download?id=7",
    )
    want = cli("tests/fixtures/docx/markitdown-test.docx").decode()
    assert md.convert(resp).markdown + "\n" == want
