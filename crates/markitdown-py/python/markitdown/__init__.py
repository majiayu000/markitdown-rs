"""markitdown's Python API, backed by markitdown-rs.

    from markitdown import MarkItDown
    result = MarkItDown().convert("report.pdf")
    print(result.markdown)

Conversions return the same Markdown as microsoft/markitdown 0.1.9.
"""

from ._markitdown_rs import __version__
from ._markitdown import MarkItDown, HTTPError
from ._base_converter import DocumentConverterResult
from ._stream_info import StreamInfo
from ._exceptions import (
    MarkItDownException,
    MissingDependencyException,
    FailedConversionAttempt,
    FileConversionException,
    UnsupportedFormatException,
)

__all__ = [
    "__version__",
    "MarkItDown",
    "DocumentConverterResult",
    "MarkItDownException",
    "MissingDependencyException",
    "FailedConversionAttempt",
    "FileConversionException",
    "UnsupportedFormatException",
    "StreamInfo",
    "HTTPError",
]
