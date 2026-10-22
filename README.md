# markitdown-rs

[![CI](https://github.com/majiayu000/markitdown-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/majiayu000/markitdown-rs/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/majiayu000/markitdown-rs)](https://github.com/majiayu000/markitdown-rs/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**markitdown-rs converts PDF, Word, PowerPoint, Excel, HTML, EPUB, Outlook
and web pages to Markdown for LLMs and RAG pipelines.** It is Microsoft's
[markitdown](https://github.com/microsoft/markitdown) CLI rewritten in Rust as
one 4.5 MB binary: the same command and the **same Markdown, byte for byte**,
without a Python environment, **23–112× faster**.

```bash
curl -L https://github.com/majiayu000/markitdown-rs/releases/latest/download/markitdown-v0.1.0-aarch64-apple-darwin.tar.gz | tar xz
./markitdown-v0.1.0-aarch64-apple-darwin/markitdown report.pdf > report.md
```

Builds for macOS (Apple silicon and Intel) and Linux x86_64 are on the
[releases page](https://github.com/majiayu000/markitdown-rs/releases); with
Rust installed, `cargo install --git https://github.com/majiayu000/markitdown-rs markitdown-rs`.

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
| Output identical to markitdown | **400 of 405 test files** | reference | 35 of 405 | not a goal |
| Install | one binary | Python + pip (328 MB venv) | `cargo install` | pip / cargo / npm / ... |
| Speed (CLI) | **23–112× markitdown** | 1× | – | – |
| Formats | markitdown's local formats + URLs | all + OCR/LLM plugins, Azure | 9 | 107, OCR, layout models |

Full feature-by-feature table, with how each number was measured:
[docs/comparison.md](docs/comparison.md).

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
[Parity testing](#parity-testing)): 407 files, the 6,705 inputs of the
html5lib tokenizer suite through the HTML converter, and 13 pages converted as
if fetched from Wikipedia, Bing and YouTube URLs.

| Format | Fixtures | Identical to Python |
|---|---:|---|
| Word `.docx` | 71 | 69 of 70; 1 known difference; 1 file Python cannot convert |
| PowerPoint `.pptx` | 44 | 44 |
| PDF | 67 | 67 |
| Excel `.xlsx` | 50 | 49; 1 file Python cannot convert |
| Excel `.xls` | 43 | 43 |
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
- **Style maps embedded by mammoth** (a `mammoth/style-map` part) are ignored.
- **Files where Python fails.** Python hits its recursion limit on tables
  nested 5000 deep; markitdown-rs stops at 512 levels of XML nesting with an
  error. Wikipedia answers Python's default `python-requests` User-Agent with
  HTTP 403; markitdown-rs sends its own and gets the page.

Not supported: YouTube transcripts and audio transcription, LLM image captions
(`llm_client`), Azure Document Intelligence / Content Understanding (`-d`,
`--use-cu`), OCR and other plugins (`-p`), and the Python API's `style_map`
argument. Like markitdown, legacy `.doc` and `.ppt` files are not converted.

## Install

Prebuilt binaries for macOS (arm64, x86_64) and Linux (x86_64) are attached to
each [release](https://github.com/majiayu000/markitdown-rs/releases), with
SHA-256 sums. From source (Rust 1.88+):

```bash
cargo install --git https://github.com/majiayu000/markitdown-rs markitdown-rs
```

or from a checkout:

```bash
cargo install --path crates/markitdown-rs
```

Both install a `markitdown` binary. Homebrew and crates.io packages are not
published yet.

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

As a library (`markitdown-core`, no network code):

```rust
use markitdown_core::{StreamInfo, convert};
let data = std::fs::read("report.pdf")?;
let md = convert(&data, &StreamInfo::from_path("report.pdf"), &Default::default())?;
```

## Benchmarks

Python markitdown 0.1.9 (CPython 3.12.12, versions pinned in `oracle/uv.lock`)
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
crates/markitdown-rs        the `markitdown` binary (URL fetching lives here)
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
