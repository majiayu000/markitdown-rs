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
`StreamInfo` and the exception classes behave as in markitdown 0.1.9.
Plugins, LLM image captions and Azure Document Intelligence are not available.
The module is named `markitdown`, so install it instead of, not next to, the
Python package of that name.

Build from a checkout with uv:

```bash
cd crates/markitdown-py
uv venv .venv && VIRTUAL_ENV=$PWD/.venv uv pip install maturin pytest
VIRTUAL_ENV=$PWD/.venv .venv/bin/maturin develop --release --uv
.venv/bin/pytest tests                     # needs target/release/markitdown (cargo build --release)
```
