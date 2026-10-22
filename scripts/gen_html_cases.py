"""Record markitdown's output for every input of html5lib-tests' tokenizer
suite (MIT), as tests/snapshots/html-cases.json.

usage: uv run --project oracle python scripts/gen_html_cases.py <html5lib-tests checkout>
"""
import io
import json
import re
import sys
from pathlib import Path

from markitdown import MarkItDown, StreamInfo

OUT = Path(__file__).resolve().parent.parent / "tests/snapshots/html-cases.json"


def undouble(s):
    return re.sub(r"\\u([0-9A-Fa-f]{4})", lambda m: chr(int(m.group(1), 16)), s)


def main(checkout):
    md = MarkItDown()
    seen, cases = set(), []
    for f in sorted(Path(checkout, "tokenizer").glob("*.test")):
        for t in json.loads(f.read_text()).get("tests", []):
            text = undouble(t["input"]) if t.get("doubleEscaped") else t["input"]
            try:
                data = text.encode("utf-8")
            except UnicodeEncodeError:
                continue  # lone surrogates cannot be stored in a file
            if data in seen:
                continue
            seen.add(data)
            case = {"source": f.name, "input": text}
            try:
                case["output"] = md.convert_stream(io.BytesIO(data), stream_info=StreamInfo(extension=".html")).markdown
            except Exception as e:  # noqa: BLE001 - recorded, not hidden
                case["error"] = type(e).__name__
            cases.append(case)
    OUT.write_text(json.dumps(cases, ensure_ascii=False, indent=0) + "\n")
    print(f"wrote {len(cases)} cases to {OUT}")


if __name__ == "__main__":
    main(sys.argv[1])
