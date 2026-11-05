# markitdown-rs

[![CI](https://github.com/majiayu000/markitdown-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/majiayu000/markitdown-rs/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/majiayu000/markitdown-rs)](https://github.com/majiayu000/markitdown-rs/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**markitdown-rs converts PDF, Word, PowerPoint, Excel, HTML, EPUB, Outlook
and web pages to Markdown for LLMs and RAG pipelines.** It is Microsoft's
[markitdown](https://github.com/microsoft/markitdown) CLI rewritten in Rust as
one 4.5 MB binary: the same command and the **same Markdown, byte for byte**,
without a Python environment. From the command line it is **usually hundreds
of times faster** (median 408× over 231 real-world files, about 15× on the
largest PDFs and spreadsheets); in process, 4–50× faster.

```bash
brew install majiayu000/tap/markitdown-rs   # or: cargo install markitdown-rs
markitdown report.pdf > report.md
```

Prebuilt binaries for macOS (Apple silicon and Intel) and Linux x86_64 are on
the [releases page](https://github.com/majiayu000/markitdown-rs/releases).
It also runs as an MCP server for AI agents (`markitdown mcp`, stdio or
HTTP) and has a Python module with markitdown's API, plugins included.

## Example

A PDF form becomes a Markdown table, exactly as markitdown renders it
(`tests/fixtures/pdf/gen-form-mixed.pdf`):

```console
$ markitdown order.pdf
Purchase order summary
| Item     | Qty | Unit price | Total |
| -------- | --- | ---------- | ----- |
| Widget 1 | 1   | 1.50       | 1.50  |
| Widget 2 | 2   | 3.00       | 6.00  |
| Widget 3 | 3   | 4.50       | 13.50 |
...
Thank you for your order.
```

URLs work too, with markitdown's Wikipedia and YouTube handling:

```console
$ markitdown https://en.wikipedia.org/wiki/Markdown | head -3
# Markdown

Plain text markup language
```

## markitdown-rs vs. the alternatives

| | markitdown-rs | [microsoft/markitdown](https://github.com/microsoft/markitdown) | [markitdown (crates.io)](https://github.com/uhobnil/markitdown-rs) | [xberg](https://github.com/xberg-io/xberg) |
|---|---|---|---|---|
| Output identical to markitdown | **401 of 406 test files** | reference | 35 of 405 | not a goal |
| Install | one binary | Python + pip (328 MB venv) | `cargo install` | pip / cargo / npm / ... |
| Speed (CLI) | **median 408× markitdown** (15–1,660×) | 1× | – | – |
| Formats | markitdown's formats + URLs; OCR/LLM plugins and Azure in the Python module | all + OCR/LLM plugins, Azure | 9 | 107, OCR, layout models |
| MCP server / Python API | `markitdown mcp` (stdio, HTTP) / `from markitdown import MarkItDown` | `markitdown-mcp` / yes | no / no | yes / yes |

Full feature-by-feature table, with how each number was measured:
[markitdown vs markitdown-rs](https://majiayu000.github.io/markitdown-rs/comparison.html).

## Why

markitdown is the de facto "documents to Markdown for LLMs" tool, but using it
from a shell, CI job or non-Python service means shipping a Python environment
with mammoth, pdfminer.six, pdfplumber, pandas, openpyxl, BeautifulSoup,
python-pptx, magika and onnxruntime. Its most requested features include brew
and binary installs ([#1179](https://github.com/microsoft/markitdown/issues/1179),
[#25](https://github.com/microsoft/markitdown/issues/25)) and lighter
dependencies ([#1234](https://github.com/microsoft/markitdown/issues/1234)).

markitdown-rs targets output compatibility. It does not try to produce
"better" Markdown: it reproduces markitdown's output, quirks included (pipes in
table cells, Heading 7–9 as plain text, pdfminer's `(cid:N)` placeholders,
pandas' `NaN` cells), so you can swap the binary without changing anything
downstream.

## Compatibility

Every fixture is converted by both implementations and compared byte for byte
in `cargo test` against Python markitdown 0.1.9 (see
[Parity testing](#parity-testing)): 408 files, the 6,705 inputs of the
html5lib tokenizer suite through the HTML converter, and 13 pages converted as
if fetched from Wikipedia, Bing and YouTube URLs.

| Format | Fixtures | Identical to Python |
|---|---:|---|
| Word `.docx` | 71 | 69 of 70; 1 known difference; 1 file Python cannot convert |
| PowerPoint `.pptx` | 44 | 44 |
| PDF | 67 | 67 |
| Excel `.xlsx` | 50 | 49; 1 file Python cannot convert |
| Excel `.xls` (Excel 3/4 and 97–2003) | 44 | 44 |
| Outlook `.msg` | 38 | 35; 3 known differences |
| HTML (and Wikipedia, Bing, YouTube pages) | 36 + 6,705 + 13 | all |
| EPUB | 6 | 6 |
| ZIP (contents converted recursively) | 3 | 3 |
| RSS / Atom | 12 | 12 |
| CSV | 11 | 11 |
| Jupyter `.ipynb` | 4 | 4 |
| Plain text, Markdown, JSON, XML | 15 | 14; 1 known difference |
| Images, audio (metadata via exiftool) | 7 | 7 |

Sources are files and stdin as in markitdown, plus `http://` / `https://`
URLs (with markitdown's `Accept` header and stream detection from the
response), `file:` and `data:` URIs.

What each converter covers:

- **DOCX** follows mammoth 1.13: headings, bold/italic/underline/strike, lists,
  tables with merged and nested cells, hyperlinks (including linked
  pictures), footnotes and endnotes, images, equations (OMML to LaTeX), text
  boxes, checkboxes and tracked changes.
- **PPTX** follows python-pptx: titles, text in reading order, groups, tables,
  pictures, charts (bar, line, pie, area, radar, scatter, bubble) and notes.
- **PDF** is a port of pdfminer.six and the parts of pdfplumber markitdown
  uses: xref tables and streams (with the fallback for broken files), object
  streams, Flate/LZW/ASCII85/ASCIIHex/RunLength with predictors, RC4 and AES
  encryption (empty user password), Type1/TrueType/Type3/CID fonts with
  ToUnicode maps, the standard 14 fonts' metrics, embedded TrueType `cmap`s and
  the predefined CJK CMaps, form XObjects, pdfminer's layout analysis, and
  markitdown's borderless-table detection on pdfplumber word positions.
- **XLSX/XLS** follow pandas' `read_excel` (openpyxl/xlrd value conversion,
  dtype inference, `to_html`), one table per sheet.
- **HTML** ports CPython's `html.parser`, BeautifulSoup's tree builder and
  markdownify with markitdown's overrides; Wikipedia, Bing and YouTube pages
  get their dedicated converters when the source URL says so.
- **MSG** reads the MAPI properties from the OLE container (code pages
  included); **EPUB** walks the OPF spine; **ZIP** converts every member.
- **Images and audio** print exiftool metadata when exiftool is installed,
  as markitdown does.
- **YouTube pages** get the video's transcript (`### Transcript`), fetched
  the way markitdown's optional youtube-transcript-api does.

Known differences:

- **PDF text-box order on rare layouts.** pdfminer breaks ties between equal
  box distances by `id()`, a CPython memory address. markitdown-rs uses
  creation order instead. Of 313 PDFs tried (all of DP-Bench, arXiv papers,
  a 10-K, CJK manuals, other projects' test files) 308 are identical, one
  polyglot file is rejected by both, and 4 differ only in where a tied box
  lands (they match a pdfminer patched to use creation order).
- **Content sniffing** uses rules instead of magika's model, so files without
  an extension (or with a wrong one) can be routed differently; charset
  detection uses chardetng instead of charset_normalizer, which disagrees on
  short legacy 8-bit text and on Outlook messages that declare the wrong code
  page (`tests/known-diffs.txt`).
- **Style maps embedded by mammoth** (a `mammoth/style-map` part) are ignored
  by the Rust converter; the Python module applies them when given a
  `style_map` (see [Python](#python)).
- **Deeply nested input.** HTML nested deeper than Python's recursion limit
  allows (493 elements) falls back to plain text exactly where markitdown
  does, with the same warning, however deep it goes. Word tables nested 5000
  deep make Python fail; markitdown-rs stops at 512 levels of XML nesting with
  an error.
- **UTF-16 HTML with a byte order mark** is decoded by its BOM. markitdown
  decodes UTF-16LE but turns UTF-16BE pages into mojibake.
- Wikipedia answers Python's default `python-requests` User-Agent with HTTP
  403; markitdown-rs sends its own and gets the page.

The command line does not do what markitdown leaves to optional Python
packages or cloud services: audio transcription (markitdown's
speech_recognition calls Google's unofficial speech API with a key built into
that library, and needs ffmpeg), LLM image captions, Azure Document
Intelligence / Content Understanding (`-d`, `--use-cu`) and plugins (`-p`).
The [Python module](#python) does all of these, and its own `markitdown`
command has the original's full set of flags. Like markitdown, legacy `.doc` and
`.ppt` files are not converted.

## Install

With Homebrew (macOS, Linux x86_64):

```bash
brew install majiayu000/tap/markitdown-rs
```

From [crates.io](https://crates.io/crates/markitdown-rs) (Rust 1.88+):

```bash
cargo install markitdown-rs
```

Prebuilt binaries for macOS (arm64, x86_64) and Linux (x86_64) are attached to
each [release](https://github.com/majiayu000/markitdown-rs/releases), with
SHA-256 sums:

```bash
curl -L https://github.com/majiayu000/markitdown-rs/releases/download/v0.2.0/markitdown-v0.2.0-aarch64-apple-darwin.tar.gz | tar xz
./markitdown-v0.2.0-aarch64-apple-darwin/markitdown --version
```

From a checkout: `cargo install --path crates/markitdown-rs`. All of these
install a binary named `markitdown`. The PyPI package (`markitdown-rs`,
the Python module) is not published yet.

## Usage

The flags mirror the Python CLI:

```text
markitdown [filename] [-o OUTPUT] [-x EXTENSION] [-m MIME_TYPE] [-c CHARSET] [--keep-data-uris]
```

```bash
markitdown report.pdf                   # Markdown on stdout
markitdown book.xlsx -o book.md         # write to a file (no trailing newline, like Python)
markitdown https://example.com/a.docx   # fetch a URL
cat report.docx | markitdown -x docx    # stdin, with an extension hint
markitdown --keep-data-uris report.docx # keep images as full base64 data URIs
```

As in markitdown, the format comes from the hints (extension, MIME type,
charset) and from the file contents, and converters are tried in markitdown's
order until one succeeds.

Limits that markitdown does not have, so that hostile files fail cleanly
instead of exhausting memory or hanging (environment variables, also
`Options` fields in the library):

| Variable | Default | |
|---|---|---|
| `MARKITDOWN_MAX_UNZIPPED_BYTES` | 1 GiB | decompressed bytes read from ZIP archives and Office packages per conversion |
| `MARKITDOWN_MAX_ZIP_DEPTH` | 10 | ZIP archives nested in ZIP archives |
| `MARKITDOWN_HTTP_TIMEOUT` | 120 | seconds per HTTP request (30 to connect); 0 waits forever |

As a library (`markitdown-core`; URL loading is the optional `fetch` feature):

```rust
use markitdown_core::{StreamInfo, convert};
let data = std::fs::read("report.pdf")?;
let md = convert(&data, &StreamInfo::from_path("report.pdf"), &Default::default())?;
```

### MCP server

`markitdown mcp` serves the [Model Context Protocol](https://modelcontextprotocol.io)
on stdio with the same tool as microsoft/markitdown-mcp:
`convert_to_markdown(uri)` for `http:`, `https:`, `file:` and `data:` URIs
(local paths work too). Logs go to stderr, so stdout carries only JSON-RPC.

Claude Code:

```bash
claude mcp add markitdown -- markitdown mcp
```

Claude Desktop (`claude_desktop_config.json`) and Cursor (`~/.cursor/mcp.json`,
or `.cursor/mcp.json` in a project) use the same block:

```json
{
  "mcpServers": {
    "markitdown": { "command": "markitdown", "args": ["mcp"] }
  }
}
```

Desktop apps may not see your shell's `PATH`; if the server does not start,
use the full path from `which markitdown` (e.g. `~/.cargo/bin/markitdown`) as
`command`.

Tool name, input and output schemas, result shape and error messages match
markitdown-mcp 0.0.1a8; `serverInfo.version` is markitdown-rs's version, and
argument validation errors leave out pydantic's details.

Over HTTP, as `markitdown-mcp --http`:

```bash
markitdown mcp --http                      # http://127.0.0.1:3001
markitdown mcp --http --host 127.0.0.1 --port 8080
```

It serves Streamable HTTP at `/mcp` (stateless, JSON responses) and the SSE
transport at `/sse` (messages to `/messages/`), checks `Host` and `Origin`
against DNS rebinding when bound to localhost, and warns when bound to
another interface: the server has no authentication.

### Python

`crates/markitdown-py` builds a Python module with markitdown's API on top of
the same converters (PyO3 + maturin). **It is not published to PyPI yet**;
install it from source with maturin
([crates/markitdown-py/README.md](crates/markitdown-py/README.md)). Its
import name is `markitdown`, the same as Microsoft's package, so it cannot be
installed in the same environment as `markitdown` from PyPI: use one or the
other.

The wheel also installs markitdown's command, `markitdown` (and
`python -m markitdown`): 0.1.9's own `__main__.py` on these converters, with
all of its flags: `-p` / `--list-plugins`, `-d` / `-e`, `--use-cu` and
`--cu-*`, and audio transcription with `markitdown-rs[audio-transcription]`.
So there are two `markitdown` commands with the same output where they
overlap:

| | Rust binary (brew, cargo, release archives) | `pip install markitdown-rs` |
|---|---|---|
| Start-up | 4 ms | the Python interpreter's (~0.1 s) |
| Flags | markitdown's offline ones, plus `mcp` | all of markitdown 0.1.9's |
| Plugins, Azure, LLM, audio transcription | no | yes |
| Needs | nothing | Python 3.10+ |

If both are installed, whichever comes first on `PATH` runs (`which -a
markitdown` lists them). To pick explicitly, run `python -m markitdown` for
the Python one, or the binary's full path (e.g. `~/.cargo/bin/markitdown`)
for the Rust one.

```python
from markitdown import MarkItDown

result = MarkItDown().convert("report.pdf")   # path, URL, file:/data: URI, stream, requests.Response
print(result.markdown)                        # or result.text_content
```

The module follows markitdown 0.1.9's own design: the built-in converters
are `DocumentConverter` classes in `markitdown.converters`, each backed by the
Rust converter of the same name, tried in markitdown's priority order. So
these work as in markitdown:

- `convert`, `convert_local`, `convert_stream`, `convert_uri`,
  `convert_response`, `StreamInfo`, the exception classes and
  `result.title`; output is byte-identical to the CLI.
- Custom converters: `register_converter(converter, priority=...)`,
  `PRIORITY_SPECIFIC_FILE_FORMAT` / `PRIORITY_GENERIC_FILE_FORMAT`,
  subclasses of the built-ins (with the `_image_to_html` hooks of the DOCX,
  PPTX and XLSX converters), `enable_builtins=False`.
- Plugins (`enable_plugins=True`, `markitdown.plugin` entry points); the
  markitdown-ocr plugin gives the same output as with markitdown.
- `llm_client` / `llm_model` / `llm_prompt` for image descriptions and slide
  picture captions; `requests_session`; `exiftool_path`; `docintel_*` and
  `cu_*` (markitdown's Azure converters, which need the Azure SDKs).
- `style_map`: as in markitdown, the document goes through mammoth with
  the caller's and the embedded style map, then through the HTML converter
  (`markitdown-rs[docx]` installs mammoth).
- Audio transcription when `speech_recognition` is installed and YouTube
  transcripts when `youtube-transcript-api` is, as in markitdown. The
  wheel has markitdown's extras for them and for Azure
  (`markitdown-rs[audio-transcription]`, `[youtube-transcription]`,
  `[az-doc-intel]`, `[az-content-understanding]`, `[all]`); see
  [crates/markitdown-py/README.md](crates/markitdown-py/README.md).

**Installing plugins.** markitdown's plugins (markitdown-ocr and others)
depend on Microsoft's `markitdown` package, so a plain `pip install
markitdown-ocr` also installs it, and it overwrites this package's
`markitdown` module: `import markitdown` then runs Microsoft's code (tens of
times slower) and uninstalling either package removes shared files. markitdown-rs
warns about this at start-up, with the fix. Install plugins without that
dependency instead:

```bash
pip install 'markitdown-rs[ocr]'          # markitdown-ocr's other dependencies
pip install --no-deps markitdown-ocr
```

With uv, drop the dependency for the whole project in `pyproject.toml`:

```toml
[tool.uv]
override-dependencies = ["markitdown; sys_platform == 'never'"]
```

or for one install: `uv pip install markitdown-rs markitdown-ocr --override
overrides.txt` with that line in `overrides.txt`. If the warning appears,
`pip uninstall -y markitdown markitdown-rs`, then install as above.

Differences: unknown keyword arguments raise `TypeError` (unless plugins are
enabled, since plugins read their own), HTTP failures without a
`requests_session` raise
`markitdown.HTTPError` (an `OSError`) instead of `requests.HTTPError`, and the
ZIP converter converts members with the built-in converters only.
`crates/markitdown-py/tests/test_api.py` runs against both implementations.

## Benchmarks

On 231 real-world files that are not part of this repository's fixtures (234
Apache Tika test documents and 10 arXiv papers, all those both
implementations convert), measured on 2026-11-05 with markitdown-rs 0.2.0 on
an Apple Silicon Mac (macOS 26.5, Python 3.12):

- CLI, a new process per file: 14.5× to 1,660× faster, median 408×; 218 of
  the 231 files are at least 100× faster. The largest inputs are where the
  gap is smallest: the Llama 2 paper (26.6 s vs 1.6 s) and Tika's
  `jxl.xls` (18.2 s vs 1.1 s) are 16× faster. Python's start-up alone is
  1.9 s; markitdown-rs answers `--version` in 4 ms.
- In process, warm, Python module against Python module: 4.7× (a large HTML
  page) to 48× (a small XLSX); 29× on the Llama 2 paper, 26× on `jxl.xls`.
- Peak memory is a quarter to a sixth of Python's: 79 MiB vs 440 MiB on the
  Llama 2 paper, 157 MiB vs 668 MiB on `jxl.xls`.

The tables below are from the repository's own benchmark corpus, generated by
`scripts/bench.sh`. Python markitdown 0.1.9 (CPython 3.12.12, versions pinned in `oracle/uv.lock`)
against `markitdown-rs` 0.1.0 (release build, rustc 1.99.1) on the same
machine, run back to back on 2026-10-22: Apple M2 Max, 12 cores, macOS 26.5.
The machine was shared with other jobs (load average 16–47), so treat wall
times as indicative; CPU times track them closely. Raw output:
[`bench/results-2026-10-22.txt`](bench/results-2026-10-22.txt).

CLI end to end, a new process per run (median of 10; Python on `large.docx`
and `report.pdf`: 3):

| File | Python wall / CPU (ms) | markitdown-rs wall / CPU (ms) | Speedup (wall) |
|---|---:|---:|---:|
| markitdown `test.docx` (136 KB) | 572.9 / 632.0 | 5.1 / 3.9 | 112× |
| `medium.docx` (50 sections) | 1,084.9 / 1,143.5 | 22.3 / 20.8 | 49× |
| `large.docx` (1000 sections) | 11,588.9 / 11,221.9 | 374.2 / 301.1 | 31× |
| markitdown `test.pptx` (278 KB) | 599.6 / 627.4 | 5.7 / 4.5 | 105× |
| `deck.pptx` (300 slides) | 1,573.5 / 1,580.5 | 69.9 / 66.6 | 23× |
| markitdown `test.pdf` (93 KB) | 649.4 / 686.1 | 7.0 / 5.2 | 93× |
| `report.pdf` (100 pages, 20 with tables) | 5,864.0 / 5,893.1 | 180.3 / 174.4 | 33× |

In process, warm, excluding Python's 412 ms import (median of 5 Python and 10
Rust runs):

| File | Python (ms) | markitdown-rs (ms) | Speedup |
|---|---:|---:|---:|
| markitdown `test.docx` | 22.9 | 2.1 | 11× |
| `medium.docx` | 507.9 | 18.1 | 28× |
| `large.docx` | 12,265.4 | 243.0 | 50× |
| markitdown `test.pptx` | 13.3 | 1.5 | 9× |
| `deck.pptx` | 919.2 | 55.2 | 17× |
| markitdown `test.pdf` | 116.4 | 2.0 | 57× |
| `report.pdf` | 7,545.4 | 157.1 | 48× |

Peak RSS: `large.docx` Python 504,446,976 bytes, markitdown-rs 130,924,544;
`report.pdf` Python 156,663,808, markitdown-rs 45,023,232. The `markitdown`
binary is 4,524,288 bytes; the virtualenv with the markitdown extras used here
is 328 MB.

All benchmark outputs were checked to be byte-identical before timing. To
reproduce (needs [uv](https://docs.astral.sh/uv/) and Rust):

```bash
scripts/bench.sh
```

It generates the corpus (`scripts/gen_bench_corpus.py`, fixed seed), builds
the release binary and prints every table above.

## Parity testing

- `oracle/` is a uv project pinning markitdown 0.1.9 and its dependencies
  (mammoth 1.13.0, pdfminer.six, pdfplumber, pandas, openpyxl, xlrd,
  markdownify 1.2.3, python-pptx 1.0.2, ...).
- `scripts/gen_snapshots.sh` runs the Python CLI on every file in
  `tests/fixtures/` and stores its stdout in `tests/snapshots/` (or the error
  when Python fails, in which case markitdown-rs only has to not crash).
  `scripts/gen_url_snapshots.py` does the same for `tests/url/cases.tsv`,
  converting fixtures with the stream info of a URL response.
- `cargo test` converts every fixture and compares with the snapshot byte for
  byte. Accepted differences are listed with a reason in
  `tests/known-diffs.txt`; the test fails if a listed file starts matching.
- CI runs the tests on Linux, macOS and Windows and regenerates the snapshots
  from the pinned Python to prove they are current.

Fixtures come from the test corpora of markitdown, mammoth, python-pptx and
Apache POI (real documents from bug reports), DP-Bench PDFs, Project Gutenberg
EPUBs and Wikipedia pages, plus generated files that each target one
construct (`scripts/gen_*_fixtures.py`); sources and licenses are in
[`tests/fixtures/SOURCES.md`](tests/fixtures/SOURCES.md).

## How it works

Each converter ports the Python libraries markitdown calls, down to their
string handling (Python's `str.strip()`, `repr(float)`, `urllib.parse`,
`html.unescape`), rather than wrapping or approximating them:

```text
crates/markitdown-core      library
  convert.rs, detect.rs     markitdown's converter loop and content sniffing
  docx/                     mammoth's reader and HTML tree, OMML to LaTeX
  pptx/                     python-pptx's object model, charts
  pdf/                      pdfminer.six (parser, fonts, CMaps, layout) and pdfplumber words
  sheet/                    openpyxl/xlrd values, pandas frames and to_html
  html/, markdownify.rs     html.parser + BeautifulSoup tree, markdownify
  youtube.rs, epub.rs, msg.rs, rss.rs, zipfile.rs, plain.rs, media.rs
crates/markitdown-rs        the `markitdown` binary and `markitdown mcp`
crates/markitdown-py        the `markitdown` Python module (PyO3, maturin)
```

PDF data tables (glyph names, standard-14 metrics, CJK CMaps) are generated
from pdfminer.six by `scripts/gen_pdf_data.py`; the CMaps are stored
compressed (0.6 MB).

## License

MIT. markitdown-rs ports behaviour from markitdown (MIT, Microsoft), mammoth
(BSD-2-Clause), python-pptx (MIT), markdownify (MIT), pdfminer.six (MIT),
pdfplumber (MIT), pandas (BSD-3-Clause), openpyxl (MIT), xlrd (BSD),
BeautifulSoup (MIT) and CPython (PSF); the OMML conversion follows
markitdown's adaptation of dwml, and the `w:sym` table is generated from
mammoth's. The CMap data comes from pdfminer.six, which derives it from
Adobe's CMap resources (BSD-3-Clause). Not affiliated with Microsoft.

- MCP Registry name: `mcp-name: io.github.majiayu000/markitdown-rs`
