# markitdown-rs

A single-binary, drop-in replacement for the [markitdown](https://github.com/microsoft/markitdown) CLI.
It prints the **same bytes** as Python markitdown 0.1.8 for `.docx` and `.pptx`,
**19–58× faster** end to end, with no Python runtime.

```console
$ markitdown report.docx > report.md        # same command, same output
```

| `large.docx` (1000 sections, 2.5 MB of Markdown) | Python markitdown 0.1.8 | markitdown-rs |
|---|---:|---:|
| Wall time, CLI | 11,377 ms | **237 ms** (48×) |
| Peak memory | 456 MB | **118 MB** (3.9× less) |
| Install size | 243 MB virtualenv | **0.85 MB** binary |

Measured on an Apple M2 Max; full numbers and method in [Benchmarks](#benchmarks).

## Why

markitdown is the de facto "documents to Markdown for LLMs" tool, but using it
from a shell, CI job or non-Python service means shipping a Python environment
with mammoth, lxml, BeautifulSoup, python-pptx, magika and onnxruntime. Its most
requested features include brew and binary installs
([#1179](https://github.com/microsoft/markitdown/issues/1179),
[#25](https://github.com/microsoft/markitdown/issues/25)) and lighter
dependencies ([#1234](https://github.com/microsoft/markitdown/issues/1234)).

markitdown-rs targets output compatibility. It does not try to produce
"better" Markdown: it reproduces markitdown's output, quirks included (pipes in
table cells, Heading 7–9 as plain text, all equations dropped when one is
unsupported), so you can swap the binary without changing anything downstream.

## Compatibility

Every fixture is converted by both implementations and compared byte for byte
in `cargo test` (see [Parity testing](#parity-testing)).

| Format | Status | Fixtures | Identical to Python |
|---|---|---:|---|
| `.docx` | supported | 70 | 67 of 68 comparable files; 1 known difference, 2 files Python cannot convert |
| `.pptx` | supported | 44 | 44 of 44 |
| PDF, XLSX, XLS, CSV, HTML, EPUB, Outlook MSG, ZIP, Jupyter, RSS, JSON/XML/text, images, audio, YouTube/Wikipedia/Bing URLs | not yet | - | - |

DOCX covers headings, bold/italic/underline/strike/super/subscript, lists
(nested, mixed, style-based numbering), tables (header rows, merged cells,
nested tables), hyperlinks (including field-code and internal links),
footnotes and endnotes, images (truncated or full data URIs), equations
(OMML to LaTeX), text boxes, checkboxes, content controls and tracked changes,
following mammoth's rules for what is kept and what is dropped.

PPTX covers slide titles, text frames in reading order (top, then left, with
positions inherited from layouts), soft line breaks, groups, tables, pictures,
charts (bar, line, pie, doughnut, area, radar, scatter, bubble) and speaker
notes.

Known differences:

- **Style maps embedded by mammoth** (a `mammoth/style-map` part, only present
  in files written by mammoth's `embed_style_map`) are ignored
  (`tests/known-diffs.txt`).
- **Files where Python fails.** A field-code `end` without a `begin` makes
  mammoth 1.11 raise `IndexError` ([#2646](https://github.com/microsoft/markitdown/issues/2646));
  markitdown-rs converts the document. Tables nested 5000 deep make Python hit
  its recursion limit; markitdown-rs stops at 512 levels of XML nesting with an
  error.
- Not implemented: LLM image captions (`llm_client`), Azure Document
  Intelligence / Content Understanding (`-d`, `--use-cu`), plugins (`-p`), and
  the Python API's `style_map` argument.

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
markitdown deck.pptx                    # Markdown on stdout
markitdown report.docx -o report.md     # write to a file (no trailing newline, like Python)
cat report.docx | markitdown -x docx    # stdin, with an extension hint
markitdown --keep-data-uris report.docx # keep images as full base64 data URIs
```

Without a hint the format is taken from the file extension, then from the
package contents.

As a library:

```rust
let data = std::fs::read("report.docx")?;
let md = markitdown_core::convert(&data, markitdown_core::Format::Docx, &Default::default())?;
```

## Benchmarks

Python markitdown 0.1.8 (CPython 3.12.12, versions pinned in `oracle/uv.lock`)
against `markitdown-rs` 0.1.0 (release build, rustc 1.97.0) on the same
machine, run back to back on 2026-10-22: Apple M2 Max, 12 cores, macOS 26.5.
The machine was shared with other jobs (load average 9–20), so treat wall
times as indicative; CPU times track them closely. Raw output:
[`bench/results-2026-10-22.txt`](bench/results-2026-10-22.txt).

CLI end to end, a new process per run (median of 10; Python on `large.docx`: 3):

| File | Python wall / CPU (ms) | markitdown-rs wall / CPU (ms) | Speedup (wall) |
|---|---:|---:|---:|
| markitdown `test.docx` (136 KB) | 252.8 / 300.0 | 4.7 / 3.4 | 54× |
| `medium.docx` (50 sections) | 710.2 / 754.3 | 21.2 / 19.7 | 33× |
| `large.docx` (1000 sections) | 11,376.9 / 11,024.8 | 237.2 / 233.8 | 48× |
| markitdown `test.pptx` (278 KB) | 272.4 / 317.8 | 4.7 / 3.6 | 58× |
| `deck.pptx` (300 slides) | 1,148.1 / 1,196.9 | 59.8 / 57.9 | 19× |

In process, warm, excluding Python's 181 ms import (median of 5 Python and 10
Rust runs):

| File | Python (ms) | markitdown-rs (ms) | Speedup |
|---|---:|---:|---:|
| markitdown `test.docx` | 23.2 | 1.1 | 21× |
| `medium.docx` | 502.6 | 17.1 | 29× |
| `large.docx` | 12,281.7 | 224.1 | 55× |
| markitdown `test.pptx` | 12.1 | 1.5 | 8× |
| `deck.pptx` | 857.1 | 54.8 | 16× |

Peak RSS converting `large.docx`: Python 455,868,416 bytes, markitdown-rs
117,915,648 bytes. The `markitdown` binary is 848,864 bytes; the virtualenv
with `markitdown[docx,pptx]` is 243 MB.

All benchmark outputs were checked to be byte-identical before timing. To
reproduce (needs [uv](https://docs.astral.sh/uv/) and Rust):

```bash
scripts/bench.sh
```

It generates the corpus (`scripts/gen_bench_corpus.py`, fixed seed), builds
the release binary and prints every table above.

## Parity testing

- `oracle/` is a uv project pinning markitdown 0.1.8 and its dependencies
  (mammoth 1.11.0, markdownify 1.2.3, python-pptx 1.0.2, ...).
- `scripts/gen_snapshots.sh` runs the Python CLI on every file in
  `tests/fixtures/` and stores its stdout in `tests/snapshots/` (or the error
  when Python fails).
- `cargo test` converts every fixture and compares with the snapshot byte for
  byte. Accepted differences are listed with a reason in
  `tests/known-diffs.txt`; the test fails if a listed file starts matching.
- CI runs the tests on Linux, macOS and Windows and regenerates the snapshots
  from the pinned Python to prove they are current.

Fixtures come from markitdown, mammoth, python-pptx and Apache POI's test
corpora (real documents from bug reports), plus generated files that target
specific upstream issues; sources and licenses are in
[`tests/fixtures/SOURCES.md`](tests/fixtures/SOURCES.md). The generators are
`scripts/gen_fixtures.py` (DOCX) and `scripts/gen_pptx_fixtures.py` (PPTX).

## How it works

The DOCX path reproduces markitdown's pipeline without its HTML round trip:
markitdown's pre-processing (OMML to LaTeX, double strike, style repairs),
mammoth's document reader and style map, mammoth's HTML tree with its
collapsing rules, then a port of markdownify with markitdown's overrides,
including Python's whitespace and `urllib.parse` semantics. The PPTX path
follows python-pptx's object model (shape factory, placeholder inheritance,
chart caches) and the same markdownify port for tables.

```text
crates/markitdown-core   library: docx/, pptx.rs, markdownify.rs, xml.rs
crates/markitdown-rs     the `markitdown` binary
```

## License

MIT. markitdown-rs ports behaviour from markitdown (MIT, Microsoft),
mammoth (BSD-2-Clause, Michael Williamson), markdownify (MIT) and
python-pptx (MIT); the OMML conversion follows markitdown's adaptation of
dwml, and the `w:sym` table is generated from mammoth's.
