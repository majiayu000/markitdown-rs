# markitdown-rs

A single-binary, drop-in replacement for the [markitdown](https://github.com/microsoft/markitdown) CLI.
It prints the **same bytes** as Python markitdown 0.1.8 for Word, PowerPoint,
PDF, Excel, HTML, EPUB, Outlook and the other formats markitdown converts
locally, **23–101×** faster end to end, with no Python runtime.

```console
$ markitdown report.pdf > report.md        # same command, same output
```

| | Python markitdown 0.1.8 | markitdown-rs |
|---|---:|---:|
| `report.pdf` (100 pages), CLI wall time | 5,987 ms | **172 ms** (35×) |
| `large.docx` (1000 sections), CLI wall time | 13,922 ms | **256 ms** (54×) |
| Peak memory, `report.pdf` / `large.docx` | 152 MB / 503 MB | **45 MB / 126 MB** |
| Install size | 329 MB virtualenv | **3.1 MB** binary |

Measured on an Apple M2 Max; full numbers and method in [Benchmarks](#benchmarks).

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
in `cargo test` (see [Parity testing](#parity-testing)): 402 files, plus the
6,705 inputs of the html5lib tokenizer suite through the HTML converter.

| Format | Fixtures | Identical to Python |
|---|---:|---|
| Word `.docx` | 70 | 67 of 68; 1 known difference; 2 files Python cannot convert |
| PowerPoint `.pptx` | 44 | 44 |
| PDF | 67 | 67 |
| Excel `.xlsx` | 50 | 49; 1 file Python cannot convert |
| Excel `.xls` | 43 | 43 |
| Outlook `.msg` | 38 | 35; 3 known differences |
| HTML (and Wikipedia, Bing result pages) | 32 + 6,705 | all |
| EPUB | 6 | 6 |
| ZIP (contents converted recursively) | 3 | 3 |
| RSS / Atom | 12 | 12 |
| CSV | 11 | 11 |
| Jupyter `.ipynb` | 4 | 4 |
| Plain text, Markdown, JSON, XML | 15 | 14; 1 known difference |
| Images, audio (metadata via exiftool) | 7 | 7 |

What each converter covers:

- **DOCX** follows mammoth: headings, bold/italic/underline/strike, lists,
  tables with merged and nested cells, hyperlinks, footnotes and endnotes,
  images, equations (OMML to LaTeX), text boxes, checkboxes and tracked changes.
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
  markdownify with markitdown's overrides; Wikipedia and Bing SERP pages get
  their dedicated converters when the input URL says so.
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
- **Files where Python fails.** markitdown-rs converts some documents Python
  rejects (e.g. a DOCX field-code `end` without a `begin`, [#2646](https://github.com/microsoft/markitdown/issues/2646))
  and stops at 512 levels of XML nesting where Python hits its recursion limit.

Not supported: audio transcription, YouTube transcripts, fetching URLs (the CLI
reads files and stdin), LLM image captions (`llm_client`), Azure Document
Intelligence / Content Understanding (`-d`, `--use-cu`), OCR and other plugins
(`-p`), and the Python API's `style_map` argument. Like markitdown, legacy
`.doc` and `.ppt` files are not converted.

## Install

From source (Rust 1.88+):

```bash
cargo install --git https://github.com/majiayu000/markitdown-rs markitdown-rs
```

or from a checkout:

```bash
cargo install --path crates/markitdown-rs
```

Both install a `markitdown` binary. Prebuilt binaries, Homebrew and crates.io
releases are not published yet.

## Usage

The flags mirror the Python CLI:

```text
markitdown [filename] [-o OUTPUT] [-x EXTENSION] [-m MIME_TYPE] [-c CHARSET] [--keep-data-uris]
```

```bash
markitdown report.pdf                   # Markdown on stdout
markitdown book.xlsx -o book.md         # write to a file (no trailing newline, like Python)
cat report.docx | markitdown -x docx    # stdin, with an extension hint
markitdown --keep-data-uris report.docx # keep images as full base64 data URIs
```

As in markitdown, the format comes from the hints (extension, MIME type,
charset) and from the file contents, and converters are tried in markitdown's
order until one succeeds.

As a library:

```rust
use markitdown_core::{StreamInfo, convert};
let data = std::fs::read("report.pdf")?;
let md = convert(&data, &StreamInfo::from_path("report.pdf"), &Default::default())?;
```

## Benchmarks

Python markitdown 0.1.8 (CPython 3.12.12, versions pinned in `oracle/uv.lock`)
against `markitdown-rs` 0.1.0 (release build, rustc 1.99.1) on the same
machine, run back to back on 2026-10-22: Apple M2 Max, 12 cores, macOS 26.5.
The machine was shared with other jobs (load average 8–18), so treat wall
times as indicative; CPU times track them closely. Raw output:
[`bench/results-2026-10-22.txt`](bench/results-2026-10-22.txt).

CLI end to end, a new process per run (median of 10; Python on `large.docx`
and `report.pdf`: 3):

| File | Python wall / CPU (ms) | markitdown-rs wall / CPU (ms) | Speedup (wall) |
|---|---:|---:|---:|
| markitdown `test.docx` (136 KB) | 512.3 / 565.4 | 5.2 / 3.9 | 98× |
| `medium.docx` (50 sections) | 988.9 / 1,035.3 | 22.3 / 20.9 | 44× |
| `large.docx` (1000 sections) | 13,921.7 / 11,395.8 | 255.7 / 253.0 | 54× |
| markitdown `test.pptx` (278 KB) | 546.5 / 602.7 | 5.7 / 4.3 | 96× |
| `deck.pptx` (300 slides) | 1,546.4 / 1,588.5 | 67.4 / 65.6 | 23× |
| markitdown `test.pdf` (93 KB) | 645.0 / 695.2 | 6.4 / 5.2 | 101× |
| `report.pdf` (100 pages, 20 with tables) | 5,986.9 / 6,011.4 | 172.4 / 170.1 | 35× |

In process, warm, excluding Python's 401 ms import (median of 5 Python and 10
Rust runs):

| File | Python (ms) | markitdown-rs (ms) | Speedup |
|---|---:|---:|---:|
| markitdown `test.docx` | 24.2 | 1.3 | 18× |
| `medium.docx` | 531.0 | 18.1 | 29× |
| `large.docx` | 10,183.5 | 246.5 | 41× |
| markitdown `test.pptx` | 25.6 | 1.6 | 16× |
| `deck.pptx` | 1,065.0 | 57.6 | 18× |
| markitdown `test.pdf` | 124.8 | 2.0 | 62× |
| `report.pdf` | 5,626.6 | 164.9 | 34× |

Peak RSS: `large.docx` Python 502,677,504 bytes, markitdown-rs 125,714,432;
`report.pdf` Python 152,322,048, markitdown-rs 44,957,696. The `markitdown`
binary is 3,098,960 bytes; the virtualenv with every markitdown extra used
here is 329 MB.

All benchmark outputs were checked to be byte-identical before timing. To
reproduce (needs [uv](https://docs.astral.sh/uv/) and Rust):

```bash
scripts/bench.sh
```

It generates the corpus (`scripts/gen_bench_corpus.py`, fixed seed), builds
the release binary and prints every table above.

## Parity testing

- `oracle/` is a uv project pinning markitdown 0.1.8 and its dependencies
  (mammoth 1.11.0, pdfminer.six, pdfplumber, pandas, openpyxl, xlrd,
  markdownify 1.2.3, python-pptx 1.0.2, ...).
- `scripts/gen_snapshots.sh` runs the Python CLI on every file in
  `tests/fixtures/` and stores its stdout in `tests/snapshots/` (or the error
  when Python fails, in which case markitdown-rs only has to not crash).
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
  epub.rs, msg.rs, rss.rs, zipfile.rs, plain.rs, media.rs
crates/markitdown-rs        the `markitdown` binary
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
Adobe's CMap resources (BSD-3-Clause).
