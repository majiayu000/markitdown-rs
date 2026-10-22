# markitdown-rs compared

How markitdown-rs stands against the tools people reach for to turn documents
into Markdown for LLMs. Repository numbers were read from the GitHub and
crates.io APIs on 2026-10-22; behaviour was checked by running each tool where
that was possible, and otherwise taken from its README (marked *README*).

## At a glance

| | **markitdown-rs** | [microsoft/markitdown](https://github.com/microsoft/markitdown) | [markitdown on crates.io](https://github.com/uhobnil/markitdown-rs) (uhobnil) | [xberg](https://github.com/xberg-io/xberg) (formerly kreuzberg) | [MDFlux](https://github.com/ibrahimqureshae/mdflux) |
|---|---|---|---|---|---|
| What it is | markitdown's CLI and converters in one Rust binary | the reference: Python library + CLI | Rust library + CLI inspired by markitdown | document-intelligence platform with a Rust core | desktop app around Python markitdown |
| GitHub stars | new | 191,550 | 76 | 9,437 | 568 |
| Latest release | v0.1.0 | v0.1.9 (2026-10-21) | 0.1.11 (2026-04-22) | v1.3.10 (2026-10-13) | v0.3.0 (2026-08-23) |
| License | MIT | MIT | MIT | MIT | MIT |
| Install | one binary (or `cargo install`) | `pip install 'markitdown[all]'` + Python 3.10+ | `cargo install markitdown` (needs `--locked` today: unlocked, its `calamine` dependency no longer compiles) | pip, npm, cargo, Go, Java and more; Docker | Windows/Linux zip; sets up its own Python on first run |
| Runtime dependencies | none | Python 3.10–3.14 and 46 packages for the docx/pptx/xlsx/xls/pdf/outlook extras, incl. magika + onnxruntime (328 MB virtualenv) | none | native library per binding; ONNX models for layout/OCR features | bundled Python + markitdown |
| Output identical to markitdown | **400 of 405 test files** (5 documented differences) | reference | 35 of 405 (164 files fail to convert) | not a goal (own Markdown) | same engine as markitdown, plus optional cleanup |
| Speed vs markitdown (CLI) | **23–112× faster** | 1× | not measured | not measured | same engine |

The "identical" row runs every fixture in this repository through each tool's
CLI and compares with Python markitdown 0.1.9's output byte for byte
(`tests/fixtures`, 405 files with a Python result). For the crates.io crate:
`markitdown 0.1.11`, installed with `cargo install --locked`.

## Formats and features

| | markitdown-rs | microsoft/markitdown | markitdown (crates.io) | xberg | MDFlux |
|---|---|---|---|---|---|
| PDF (text layer) | yes, pdfminer-identical | yes (pdfminer + pdfplumber) | yes (plain text) | yes, with layout models | yes (markitdown) |
| PDF tables without borders | yes, markitdown's detection | yes | no | yes (table models) | yes (markitdown) |
| Scanned PDFs (OCR) | no | plugin (`markitdown-ocr`, needs an LLM) | no | yes (Tesseract, PaddleOCR, ...) | yes, built in |
| Word `.docx` | yes | yes | yes | yes | yes |
| PowerPoint `.pptx` | yes | yes | yes | yes | yes |
| Excel `.xlsx` / `.xls` | yes / yes | yes / yes | yes / no *README* | yes | yes |
| HTML, Wikipedia, Bing pages | yes | yes | HTML only | yes | HTML |
| EPUB | yes | yes | no *README* | yes | yes |
| Outlook `.msg` | yes | yes | no *README* | yes (email) | no *README* |
| CSV, JSON, XML, RSS/Atom, Jupyter | yes | yes | CSV, XML, RSS *README* | yes | CSV, JSON, XML |
| ZIP (converts contents) | yes | yes | yes | yes, plus tar/7z | no *README* |
| Images, audio | metadata (exiftool) | metadata; audio transcription | images *README* | OCR; transcription (feature) | images, audio |
| http(s) URLs | yes | yes | no | yes (feature) | no *README* |
| YouTube pages | metadata and description | metadata, description, transcript (extra) | no | not stated | no |
| LLM image captions | no | yes (`llm_client`) | yes (`llm_client`) *README* | yes (VLM backends) | AI cleanup pass |
| Azure Document Intelligence | no | yes | no | not stated | no |
| Plugins | no | yes | no | yes (OCR backends) | no |
| Python API | no (CLI or Rust library) | yes | no | yes | no |
| Rust library | yes (`markitdown-core`) | no | yes | yes | no |
| MCP server | no | yes (`markitdown-mcp`) | no | yes | no |
| GUI | no | no | no | no | yes |

## When to use which

- **You call `markitdown` from a shell, CI job, container or non-Python
  service** and want the same Markdown without shipping Python: markitdown-rs.
- **You need OCR, LLM captions, Azure or plugins**: Python markitdown (or
  MDFlux for a desktop app with OCR).
- **You need layout-model PDF parsing, many more formats or bindings in other
  languages**, and do not need markitdown's exact output: xberg.

## Method

- Stars and dates: `gh api repos/<owner>/<repo>` and `/releases/latest`,
  crates.io API, 2026-10-22.
- Parity: `cargo test` in this repository for markitdown-rs; for the crates.io
  crate, the same fixtures through its CLI against the same snapshots.
- Speed: `scripts/bench.sh`, numbers in the [README](../README.md#benchmarks).
