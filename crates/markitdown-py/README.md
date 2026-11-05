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
markitdown 0.1.9, and so does `style_map` (through mammoth, with the
`docx` extra). Unknown keyword arguments are ignored, as in markitdown,
with a warning naming them (unless plugins are enabled). The wheel installs markitdown's `markitdown` command too (also
`python -m markitdown`), with all of 0.1.9's flags.
The module is named `markitdown`, so install it instead of, not next to, the
Python package of that name.

Build a wheel from a checkout with uv (into `target/wheels/`):

```bash
cd crates/markitdown-py
uv venv .venv && VIRTUAL_ENV=$PWD/.venv uv pip install maturin
VIRTUAL_ENV=$PWD/.venv .venv/bin/maturin build --release
```

The optional features take markitdown's extras, by the same names:

```bash
uv pip install 'target/wheels/markitdown_rs-0.2.0-cp310-abi3-macosx_11_0_arm64.whl[all]'
```

| Extra | Installs | For |
|---|---|---|
| `audio-transcription` | SpeechRecognition, pydub (and audioop-lts on Python 3.13+) | `### Audio Transcript` for wav, mp3, m4a (mp3 and m4a need ffmpeg) |
| `youtube-transcription` | youtube-transcript-api | `### Transcript` for YouTube pages |
| `az-doc-intel` | azure-ai-documentintelligence, azure-identity | `docintel_endpoint=` |
| `az-content-understanding` | azure-ai-contentunderstanding, azure-identity | `cu_endpoint=` |
| `docx` | mammoth, lxml, beautifulsoup4, defusedxml | `style_map=` |
| `mcp` | the MCP SDK | `markitdown-mcp` and `markitdown mcp` (markitdown-mcp 0.0.1a8's server, with plugins) |
| `ocr` | markitdown-ocr's dependencies except `markitdown` | then `pip install --no-deps markitdown-ocr` |
| `all` | all of the above | |

`pptx`, `xlsx`, `xls`, `pdf` and `outlook` exist but install nothing: those
converters are built in.

Plugins depend on Microsoft's `markitdown`, which would overwrite this
package's `markitdown` module; install them with `--no-deps` (or, with uv,
`override-dependencies = ["markitdown; sys_platform == 'never'"]`). The
package warns, when `markitdown` is imported, if both are installed or if
its `markitdown` module was removed or replaced.

Or install it into a development environment and run the tests:

```bash
cd crates/markitdown-py
uv venv .venv && VIRTUAL_ENV=$PWD/.venv uv pip install maturin pytest
VIRTUAL_ENV=$PWD/.venv .venv/bin/maturin develop --release --uv
.venv/bin/pytest tests                     # needs target/release/markitdown (cargo build --release)
```
