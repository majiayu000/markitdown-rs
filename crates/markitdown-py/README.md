# markitdown-rs (Python)

Microsoft's [markitdown](https://github.com/microsoft/markitdown) Python API,
backed by [markitdown-rs](https://github.com/majiayu000/markitdown-rs): the
same `markitdown` module and the same Markdown, byte for byte, from one native
extension instead of pdfminer, pandas, mammoth, magika and onnxruntime.

```python
from markitdown import MarkItDown

md = MarkItDown()
result = md.convert("report.pdf")          # path, Path, URL, file:/data: URI, stream or requests.Response
print(result.markdown)                     # also result.text_content
```

`convert`, `convert_local`, `convert_stream`, `convert_uri`, `convert_response`,
`StreamInfo`, the exception classes, custom converters (`register_converter`,
the `markitdown.converters` classes), plugins (`enable_plugins=True`, e.g.
markitdown-ocr), `llm_client` image captions, `requests_session`,
`exiftool_path` and the Azure `docintel_*` / `cu_*` options behave as in
markitdown 0.1.9. `style_map` raises `NotImplementedError`; unknown keyword
arguments raise `TypeError` unless plugins are enabled.
The module is named `markitdown`, so install it instead of, not next to, the
Python package of that name.

Build a wheel from a checkout with uv (into `target/wheels/`):

```bash
cd crates/markitdown-py
uv venv .venv && VIRTUAL_ENV=$PWD/.venv uv pip install maturin
VIRTUAL_ENV=$PWD/.venv .venv/bin/maturin build --release
```

Or install it into a development environment and run the tests:

```bash
cd crates/markitdown-py
uv venv .venv && VIRTUAL_ENV=$PWD/.venv uv pip install maturin pytest
VIRTUAL_ENV=$PWD/.venv .venv/bin/maturin develop --release --uv
.venv/bin/pytest tests                     # needs target/release/markitdown (cargo build --release)
```
