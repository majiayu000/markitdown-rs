# Fixture sources

Every fixture is either published under an open-source license or generated
by a script in this repository. The file name prefix says where it came from.

| Prefix | Source | License | Version |
|---|---|---|---|
| `markitdown-` | [microsoft/markitdown](https://github.com/microsoft/markitdown) `packages/markitdown/tests/test_files/` | MIT | 69e2087 (2026-10-20) |
| `mammoth-` | [mwilliamson/python-mammoth](https://github.com/mwilliamson/python-mammoth) `tests/test-data/` | BSD-2-Clause | f3b7b9f (2026-09-26) |
| `python-pptx-` | [scanny/python-pptx](https://github.com/scanny/python-pptx) `tests/test_files/` | MIT | 278b47b (2024-08-07) |
| `poi-` | [apache/poi](https://github.com/apache/poi) `test-data/document/`, `test-data/slideshow/` (files under 400 KB) | Apache-2.0 | d66b912 (2026-10-21) |
| `wikipedia-` | English Wikipedia articles (Markdown, Rust (programming language), Japanese language), saved HTML | CC BY-SA 4.0 | retrieved 2026-10-22 |
| `gen-` | `scripts/gen_fixtures.py` (DOCX), `scripts/gen_pptx_fixtures.py` (PPTX), `scripts/gen_text_fixtures.py` (HTML, text, CSV, notebooks) | this repository | - |

`tests/snapshots/html-cases.json` holds every input of the tokenizer suite of
[html5lib-tests](https://github.com/html5lib/html5lib-tests) (MIT, c777c40,
2026-10-01) with Python markitdown's output for it
(`scripts/gen_html_cases.py`).

The `poi-` files are real-world documents attached to Apache POI bug reports
and test cases (Word, PowerPoint, LibreOffice and other producers).

Benchmark inputs are not checked in; `scripts/bench.sh` generates them.
