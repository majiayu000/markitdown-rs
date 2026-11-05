"""markitdown's Python API, backed by markitdown-rs.

    from markitdown import MarkItDown
    result = MarkItDown().convert("report.pdf")
    print(result.markdown)

Conversions return the same Markdown as microsoft/markitdown 0.1.9.
"""

try:  # installed by the wheel (and run from markitdown_rs.pth)
    import markitdown_rs_guard  # noqa: F401
except ImportError:  # a development install
    pass

from .__about__ import __version__
from ._markitdown import (
    MarkItDown,
    HTTPError,
    PRIORITY_SPECIFIC_FILE_FORMAT,
    PRIORITY_GENERIC_FILE_FORMAT,
)
from ._base_converter import DocumentConverterResult, DocumentConverter
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
    "DocumentConverter",
    "DocumentConverterResult",
    "MarkItDownException",
    "MissingDependencyException",
    "FailedConversionAttempt",
    "FileConversionException",
    "UnsupportedFormatException",
    "StreamInfo",
    "PRIORITY_SPECIFIC_FILE_FORMAT",
    "PRIORITY_GENERIC_FILE_FORMAT",
    "HTTPError",
]
