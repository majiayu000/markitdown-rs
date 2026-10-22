"""Write tests/url/<name>.md: Python markitdown's output for each fixture in
tests/url/cases.tsv as if it had been fetched from the given URL (the
stream info convert_response builds from the response).

usage: uv run --project oracle python scripts/gen_url_snapshots.py
"""

import io
from pathlib import Path

from markitdown import MarkItDown, StreamInfo

ROOT = Path(__file__).resolve().parent.parent
md = MarkItDown()
for line in (ROOT / "tests/url/cases.tsv").read_text().splitlines():
    if not line.strip() or line.startswith("#"):
        continue
    name, fixture, url, mimetype, charset = (line.split("\t") + [""] * 5)[:5]
    info = StreamInfo(url=url, mimetype=mimetype or None, charset=charset or None)
    data = (ROOT / "tests/fixtures" / fixture).read_bytes()
    out = md.convert_stream(io.BytesIO(data), stream_info=info).markdown
    (ROOT / "tests/url" / f"{name}.md").write_text(out, encoding="utf-8")
    print(name, len(out))
