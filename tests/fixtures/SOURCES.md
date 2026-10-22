# Fixture sources

Every fixture is either published under an open-source license or generated
by a script in this repository. The file name prefix says where it came from.

| Prefix | Source | License | Version |
|---|---|---|---|
| `markitdown-` | [microsoft/markitdown](https://github.com/microsoft/markitdown) `packages/markitdown/tests/test_files/`, and the text PDFs of `packages/markitdown-ocr/tests/ocr_test_data/` | MIT | 69e2087 (2026-10-20) |
| `mammoth-` | [mwilliamson/python-mammoth](https://github.com/mwilliamson/python-mammoth) `tests/test-data/` | BSD-2-Clause | f3b7b9f (2026-09-26) |
| `python-pptx-` | [scanny/python-pptx](https://github.com/scanny/python-pptx) `tests/test_files/` | MIT | 278b47b (2024-08-07) |
| `poi-` | [apache/poi](https://github.com/apache/poi) `test-data/document/`, `test-data/slideshow/` (files under 400 KB), `test-data/spreadsheet/` (every k-th name, under 300 KB), `test-data/hsmf/` (Outlook messages) | Apache-2.0 | d66b912 (2026-10-21) |
| `gutenberg-` | Project Gutenberg EPUBs #11 (Alice's Adventures in Wonderland) and #1661 (The Adventures of Sherlock Holmes), no-images editions | public domain in the USA | retrieved 2026-10-22 |
| `dpbench-` | [DP-Bench](https://huggingface.co/datasets/upstage/dp-bench) single-page PDFs (every third one under 110 KB) as shipped in [opendataloader-bench](https://github.com/opendataloader-project/opendataloader-bench) `pdfs/` | MIT (DP-Bench), Apache-2.0 (opendataloader-bench) | retrieved 2026-10-21 |
| `wikipedia-` | English Wikipedia articles (Markdown, Rust (programming language), Japanese language), saved HTML | CC BY-SA 4.0 | retrieved 2026-10-22 |
| `gen-` | `scripts/gen_fixtures.py` (DOCX), `scripts/gen_pptx_fixtures.py` (PPTX), `scripts/gen_text_fixtures.py` (HTML, text, CSV, notebooks, feeds), `scripts/gen_sheet_fixtures.py` (XLSX), `scripts/gen_pdf_fixtures.py` (PDF) | this repository | - |

`tests/snapshots/html-cases.json` holds every input of the tokenizer suite of
[html5lib-tests](https://github.com/html5lib/html5lib-tests) (MIT, c777c40,
2026-10-01) with Python markitdown's output for it
(`scripts/gen_html_cases.py`).

The `poi-` files are real-world documents attached to Apache POI bug reports
and test cases (Word, PowerPoint, LibreOffice and other producers).

`tests/exiftool-stub.sh` stands in for exiftool (fixed JSON output) so the
image and audio metadata formatting can be compared without installing it.

Benchmark inputs are not checked in; `scripts/bench.sh` generates them.
