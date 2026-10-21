# Fixture sources

Every fixture is either published under an open-source license or generated
by a script in this repository. The file name prefix says where it came from.

| Prefix | Source | License | Version |
|---|---|---|---|
| `markitdown-` | [microsoft/markitdown](https://github.com/microsoft/markitdown) `packages/markitdown/tests/test_files/` | MIT | 69e2087 (2026-10-20) |
| `mammoth-` | [mwilliamson/python-mammoth](https://github.com/mwilliamson/python-mammoth) `tests/test-data/` | BSD-2-Clause | f3b7b9f (2026-09-26) |
| `python-pptx-` | [scanny/python-pptx](https://github.com/scanny/python-pptx) `tests/test_files/` | MIT | 278b47b (2024-08-07) |
| `poi-` | [apache/poi](https://github.com/apache/poi) `test-data/document/`, `test-data/slideshow/` (files under 400 KB) | Apache-2.0 | d66b912 (2026-10-21) |
| `gen-` | `scripts/gen_fixtures.py` (hand-written OOXML, deterministic) | this repository | - |

The `poi-` files are real-world documents attached to Apache POI bug reports
and test cases (Word, PowerPoint, LibreOffice and other producers).

Benchmark inputs are not checked in; `scripts/bench.sh` generates them.
