# The MarkItDown class of microsoft/markitdown 0.1.9 (MIT), with the
# built-in converters running in markitdown-rs. Converter registration and
# trial order, plugins and source handling (paths, URIs, streams, requests
# responses) follow the original line by line.
import io
import os
import re
import shutil
import sys
import traceback
from dataclasses import dataclass
from email.message import Message
from email.utils import collapse_rfc2231_value
from importlib.metadata import entry_points
from pathlib import Path
from typing import Any, BinaryIO, Dict, List, Optional, Union
from urllib.parse import urlparse
from warnings import warn

from . import _markitdown_rs as _native
from ._base_converter import DocumentConverter, DocumentConverterResult
from ._exceptions import (
    FailedConversionAttempt,
    FileConversionException,
    UnsupportedFormatException,
)
from ._stream_info import StreamInfo
from .converters import (
    ContentUnderstandingConverter,
    DocumentIntelligenceConverter,
)
from .converters import _native as _builtins

_URI_SCHEMES = ("http", "https", "file", "data")


class HTTPError(OSError):
    """An HTTP error status while fetching a URI (markitdown raises
    requests.HTTPError here)."""

    def __init__(self, status: int, url: str):
        super().__init__(f"HTTP status {status} for url: {url}")
        self.status = status
        self.url = url


def _get_content_disposition_filename(content_disposition: str) -> Optional[str]:
    message = Message()
    message["content-disposition"] = content_disposition

    fallback_filename: Optional[str] = None
    extended_filename: Optional[str] = None
    for key, value in message.get_params(header="content-disposition", unquote=True):
        if key != "filename":
            continue
        if isinstance(value, tuple):
            extended_filename = collapse_rfc2231_value(value)
        elif fallback_filename is None:
            fallback_filename = value

    return extended_filename or fallback_filename


PRIORITY_SPECIFIC_FILE_FORMAT = (
    0.0  # e.g., .docx, .pdf, .xlsx, Or specific pages, e.g., wikipedia
)
PRIORITY_GENERIC_FILE_FORMAT = (
    10.0  # Near catch-all converters for mimetypes like text/*, etc.
)

# markitdown registers these with PRIORITY_GENERIC_FILE_FORMAT
_GENERIC_CONVERTERS = ("HtmlConverter", "ZipConverter", "PlainTextConverter")

# the keyword arguments MarkItDown() and enable_builtins() read
_KNOWN_KWARGS = frozenset(
    [
        "requests_session",
        "llm_client",
        "llm_model",
        "llm_prompt",
        "exiftool_path",
        "style_map",
        "docintel_endpoint",
        "docintel_credential",
        "docintel_file_types",
        "docintel_api_version",
        "cu_endpoint",
        "cu_credential",
        "cu_analyzer_id",
        "cu_file_types",
    ]
)

_plugins: Union[None, List[Any]] = None  # If None, plugins have not been loaded yet.


def _load_plugins() -> Union[None, List[Any]]:
    """Lazy load plugins, exiting early if already loaded."""
    global _plugins

    if _plugins is not None:
        return _plugins

    _plugins = []
    for entry_point in entry_points(group="markitdown.plugin"):
        try:
            _plugins.append(entry_point.load())
        except Exception:
            tb = traceback.format_exc()
            warn(f"Plugin '{entry_point.name}' failed to load ... skipping:\n{tb}")

    return _plugins


@dataclass(kw_only=True, frozen=True)
class ConverterRegistration:
    """A registration of a converter with its priority and other metadata."""

    converter: DocumentConverter
    priority: float


class MarkItDown:
    """(In preview) An extremely simple text-based document reader, suitable for LLM use.
    This reader will convert common file-types or webpages to Markdown."""

    def __init__(
        self,
        *,
        enable_builtins: Union[None, bool] = None,
        enable_plugins: Union[None, bool] = None,
        **kwargs,
    ):
        # plugins read keyword arguments of their own
        if not enable_plugins:
            for key in kwargs:
                if key not in _KNOWN_KWARGS:
                    raise TypeError(
                        f"MarkItDown.__init__() got an unexpected keyword argument '{key}'"
                    )

        self._builtins_enabled = False
        self._plugins_enabled = False

        # None: markitdown-rs fetches http(s) URIs itself, with markitdown's
        # default Accept header
        self._requests_session = kwargs.get("requests_session")

        self._llm_client: Any = None
        self._llm_model: Union[str | None] = None
        self._llm_prompt: Union[str | None] = None
        self._exiftool_path: Union[str | None] = None
        self._style_map: Union[str | None] = None

        self._converters: List[ConverterRegistration] = []

        if (
            enable_builtins is None or enable_builtins
        ):  # Default to True when not specified
            self.enable_builtins(**kwargs)

        if enable_plugins:
            self.enable_plugins(**kwargs)

    def enable_builtins(self, **kwargs) -> None:
        """
        Enable and register built-in converters.
        Built-in converters are enabled by default.
        This method should only be called once, if built-ins were initially disabled.
        """
        if not self._builtins_enabled:
            self._llm_client = kwargs.get("llm_client")
            self._llm_model = kwargs.get("llm_model")
            self._llm_prompt = kwargs.get("llm_prompt")
            self._exiftool_path = kwargs.get("exiftool_path")
            self._style_map = kwargs.get("style_map")
            if self._style_map:
                raise NotImplementedError(
                    "markitdown-rs does not support custom mammoth style maps (style_map)"
                )

            if self._exiftool_path is None:
                self._exiftool_path = os.getenv("EXIFTOOL_PATH")

            if self._exiftool_path is None:
                candidate = shutil.which("exiftool")
                if candidate:
                    candidate = os.path.abspath(candidate)
                    if any(
                        d == os.path.dirname(candidate)
                        for d in [
                            "/usr/bin",
                            "/usr/local/bin",
                            "/opt",
                            "/opt/bin",
                            "/opt/local/bin",
                            "/opt/homebrew/bin",
                            "C:\\Windows\\System32",
                            "C:\\Program Files",
                            "C:\\Program Files (x86)",
                        ]
                    ):
                        self._exiftool_path = candidate

            # registered in reverse, so the newest (first tried) comes first
            for name in reversed(_native.converter_names()):
                priority = (
                    PRIORITY_GENERIC_FILE_FORMAT
                    if name in _GENERIC_CONVERTERS
                    else PRIORITY_SPECIFIC_FILE_FORMAT
                )
                self.register_converter(getattr(_builtins, name)(), priority=priority)

            docintel_endpoint = kwargs.get("docintel_endpoint")
            if docintel_endpoint is not None:
                docintel_args: Dict[str, Any] = {}
                docintel_args["endpoint"] = docintel_endpoint

                docintel_credential = kwargs.get("docintel_credential")
                if docintel_credential is not None:
                    docintel_args["credential"] = docintel_credential

                docintel_types = kwargs.get("docintel_file_types")
                if docintel_types is not None:
                    docintel_args["file_types"] = docintel_types

                docintel_version = kwargs.get("docintel_api_version")
                if docintel_version is not None:
                    docintel_args["api_version"] = docintel_version

                self.register_converter(
                    DocumentIntelligenceConverter(**docintel_args),
                )

            cu_endpoint = kwargs.get("cu_endpoint")
            if cu_endpoint is not None:
                cu_args: Dict[str, Any] = {}
                cu_args["endpoint"] = cu_endpoint

                cu_credential = kwargs.get("cu_credential")
                if cu_credential is not None:
                    cu_args["credential"] = cu_credential

                cu_analyzer_id = kwargs.get("cu_analyzer_id")
                if cu_analyzer_id is not None:
                    cu_args["analyzer_id"] = cu_analyzer_id

                cu_file_types = kwargs.get("cu_file_types")
                if cu_file_types is not None:
                    cu_args["file_types"] = cu_file_types

                self.register_converter(
                    ContentUnderstandingConverter(**cu_args),
                )

            self._builtins_enabled = True
        else:
            warn("Built-in converters are already enabled.", RuntimeWarning)

    def enable_plugins(self, **kwargs) -> None:
        """
        Enable and register converters provided by plugins.
        Plugins are disabled by default.
        This method should only be called once, if plugins were initially disabled.
        """
        if not self._plugins_enabled:
            plugins = _load_plugins()
            assert plugins is not None
            for plugin in plugins:
                try:
                    plugin.register_converters(self, **kwargs)
                except Exception:
                    tb = traceback.format_exc()
                    warn(f"Plugin '{plugin}' failed to register converters:\n{tb}")
            self._plugins_enabled = True
        else:
            warn("Plugin converters are already enabled.", RuntimeWarning)

    def convert(
        self,
        source: Union[str, Path, BinaryIO, Any],
        *,
        stream_info: Optional[StreamInfo] = None,
        **kwargs: Any,
    ) -> DocumentConverterResult:
        """
        Args:
            - source: can be a path (str or Path), url, or a requests.response object
            - stream_info: optional stream info to use for the conversion. If None, infer from source
            - kwargs: additional arguments to pass to the converter
        """
        if isinstance(source, str):
            scheme = urlparse(source.strip()).scheme.lower()
            if scheme in _URI_SCHEMES:
                _kwargs = {k: v for k, v in kwargs.items()}
                if "url" in _kwargs:
                    _kwargs["mock_url"] = _kwargs["url"]
                    del _kwargs["url"]
                return self.convert_uri(source, stream_info=stream_info, **_kwargs)
            else:
                return self.convert_local(source, stream_info=stream_info, **kwargs)
        elif isinstance(source, Path):
            return self.convert_local(source, stream_info=stream_info, **kwargs)
        elif hasattr(source, "iter_content") and hasattr(source, "headers"):
            return self.convert_response(source, stream_info=stream_info, **kwargs)
        elif (
            hasattr(source, "read")
            and callable(source.read)
            and not isinstance(source, io.TextIOBase)
        ):
            return self.convert_stream(source, stream_info=stream_info, **kwargs)
        else:
            raise TypeError(
                f"Invalid source type: {type(source)}. Expected str, requests.Response, BinaryIO."
            )

    def convert_local(
        self,
        path: Union[str, Path],
        *,
        stream_info: Optional[StreamInfo] = None,
        file_extension: Optional[str] = None,  # Deprecated -- use stream_info
        url: Optional[str] = None,  # Deprecated -- use stream_info
        **kwargs: Any,
    ) -> DocumentConverterResult:
        if isinstance(path, Path):
            path = str(path)

        base_guess = StreamInfo(
            local_path=path,
            extension=os.path.splitext(path)[1],
            filename=os.path.basename(path),
        )
        if stream_info is not None:
            base_guess = base_guess.copy_and_update(stream_info)
        if file_extension is not None:
            base_guess = base_guess.copy_and_update(extension=file_extension)
        if url is not None:
            base_guess = base_guess.copy_and_update(url=url)

        with open(path, "rb") as fh:
            data = fh.read()
        return self._convert(data, base_guess, **kwargs)

    def convert_stream(
        self,
        stream: BinaryIO,
        *,
        stream_info: Optional[StreamInfo] = None,
        file_extension: Optional[str] = None,  # Deprecated -- use stream_info
        url: Optional[str] = None,  # Deprecated -- use stream_info
        **kwargs: Any,
    ) -> DocumentConverterResult:
        base_guess = None
        if stream_info is not None or file_extension is not None or url is not None:
            base_guess = StreamInfo() if stream_info is None else stream_info
            if file_extension is not None:
                base_guess = base_guess.copy_and_update(extension=file_extension)
            if url is not None:
                base_guess = base_guess.copy_and_update(url=url)
        return self._convert(stream.read(), base_guess or StreamInfo(), **kwargs)

    def convert_uri(
        self,
        uri: str,
        *,
        stream_info: Optional[StreamInfo] = None,
        file_extension: Optional[str] = None,  # Deprecated -- use stream_info
        mock_url: Optional[str] = None,  # Mock the request as if it came from a different URL
        **kwargs: Any,
    ) -> DocumentConverterResult:
        uri = uri.strip()
        scheme = urlparse(uri).scheme.lower()
        if scheme not in _URI_SCHEMES:
            raise ValueError(
                f"Unsupported URI scheme: {uri.split(':')[0]}. Supported schemes are: file:, data:, http:, https:"
            )
        if scheme in ("http", "https") and self._requests_session is not None:
            response = self._requests_session.get(uri, stream=True)
            response.raise_for_status()
            return self.convert_response(
                response,
                stream_info=stream_info,
                file_extension=file_extension,
                url=mock_url,
                **kwargs,
            )
        try:
            data, info = _native.load_uri(uri)
        except OSError as exc:
            if exc.args and exc.args[0] == "http":
                raise HTTPError(exc.args[1], exc.args[2]) from None
            if exc.args and exc.args[0] == "request":
                raise ConnectionError(f"{exc.args[2]}: {exc.args[1]}") from None
            raise
        base_guess = StreamInfo(**info)
        if stream_info is not None:
            base_guess = base_guess.copy_and_update(stream_info)
        if file_extension is not None:
            base_guess = base_guess.copy_and_update(extension=file_extension)
        if mock_url is not None:
            base_guess = base_guess.copy_and_update(url=mock_url)
        return self._convert(data, base_guess, **kwargs)

    def convert_url(
        self, url: str, **kwargs: Any
    ) -> DocumentConverterResult:  # TODO: fix kwargs type
        """Alias for convert_uri()"""
        return self.convert_uri(url, **kwargs)

    def convert_response(
        self,
        response: Any,
        *,
        stream_info: Optional[StreamInfo] = None,
        file_extension: Optional[str] = None,  # Deprecated -- use stream_info
        url: Optional[str] = None,  # Deprecated -- use stream_info
        **kwargs: Any,
    ) -> DocumentConverterResult:
        """Convert a requests.Response (or anything with .headers, .url and
        .iter_content())."""
        mimetype: Optional[str] = None
        charset: Optional[str] = None
        if "content-type" in response.headers:
            parts = response.headers["content-type"].split(";")
            mimetype = parts.pop(0).strip()
            for part in parts:
                if part.strip().startswith("charset="):
                    _charset = part.split("=")[1].strip()
                    if len(_charset) > 0:
                        charset = _charset

        filename: Optional[str] = None
        extension: Optional[str] = None
        if "content-disposition" in response.headers:
            filename = _get_content_disposition_filename(
                response.headers["content-disposition"]
            )
            if filename is not None:
                _, _extension = os.path.splitext(filename)
                if len(_extension) > 0:
                    extension = _extension

        if filename is None:
            parsed_url = urlparse(response.url)
            _, _extension = os.path.splitext(parsed_url.path)
            if len(_extension) > 0:  # Looks like this might be a file!
                filename = os.path.basename(parsed_url.path)
                extension = _extension

        base_guess = StreamInfo(
            mimetype=mimetype,
            charset=charset,
            filename=filename,
            extension=extension,
            url=response.url,
        )
        if stream_info is not None:
            base_guess = base_guess.copy_and_update(stream_info)
        if file_extension is not None:
            base_guess = base_guess.copy_and_update(extension=file_extension)
        if url is not None:
            base_guess = base_guess.copy_and_update(url=url)

        data = b"".join(response.iter_content(chunk_size=65536))
        return self._convert(data, base_guess, **kwargs)

    def register_page_converter(self, converter: DocumentConverter) -> None:
        """DEPRECATED: Use register_converter instead."""
        warn(
            "register_page_converter is deprecated. Use register_converter instead.",
            DeprecationWarning,
        )
        self.register_converter(converter)

    def register_converter(
        self,
        converter: DocumentConverter,
        *,
        priority: float = PRIORITY_SPECIFIC_FILE_FORMAT,
    ) -> None:
        """
        Register a DocumentConverter with a given priority.

        Priorities work as follows: By default, most converters get priority
        PRIORITY_SPECIFIC_FILE_FORMAT (== 0). The exception
        is the PlainTextConverter, HtmlConverter, and ZipConverter, which get
        priority PRIORITY_GENERIC_FILE_FORMAT (== 10), with lower values
        being tried first (i.e., higher priority).

        Just prior to conversion, the converters are sorted by priority, using
        a stable sort. This means that converters with the same priority will
        remain in the same order, with the most recently registered converters
        appearing first.
        """
        self._converters.insert(
            0, ConverterRegistration(converter=converter, priority=priority)
        )

    def _convert(
        self, data: bytes, base_guess: StreamInfo, **kwargs: Any
    ) -> DocumentConverterResult:
        stream_info_guesses = [
            StreamInfo(
                mimetype=g[0],
                extension=g[1],
                charset=g[2],
                filename=g[3],
                local_path=g[4],
                url=g[5],
            )
            for g in _native.guesses(
                data,
                (
                    base_guess.mimetype,
                    base_guess.extension,
                    base_guess.charset,
                    base_guess.filename,
                    base_guess.local_path,
                    base_guess.url,
                ),
            )
        ]
        file_stream = _builtins._SharedBytes(data)

        res: Union[None, DocumentConverterResult] = None

        # Keep track of which converters throw exceptions
        failed_attempts: List[FailedConversionAttempt] = []

        # Create a copy of the page_converters list, sorted by priority.
        # We do this with each call to _convert because the priority of converters may change between calls.
        # The sort is guaranteed to be stable, so converters with the same priority will remain in the same order.
        sorted_registrations = sorted(self._converters, key=lambda x: x.priority)

        # Remember the initial stream position so that we can return to it
        cur_pos = file_stream.tell()

        for stream_info in stream_info_guesses + [StreamInfo()]:
            for converter_registration in sorted_registrations:
                converter = converter_registration.converter
                # Sanity check -- make sure the cur_pos is still the same
                assert (
                    cur_pos == file_stream.tell()
                ), "File stream position should NOT change between guess iterations"

                _kwargs = {k: v for k, v in kwargs.items()}

                # Copy any additional global options
                if "llm_client" not in _kwargs and self._llm_client is not None:
                    _kwargs["llm_client"] = self._llm_client

                if "llm_model" not in _kwargs and self._llm_model is not None:
                    _kwargs["llm_model"] = self._llm_model

                if "llm_prompt" not in _kwargs and self._llm_prompt is not None:
                    _kwargs["llm_prompt"] = self._llm_prompt

                if "style_map" not in _kwargs and self._style_map is not None:
                    _kwargs["style_map"] = self._style_map

                if "exiftool_path" not in _kwargs and self._exiftool_path is not None:
                    _kwargs["exiftool_path"] = self._exiftool_path

                # Add the list of converters for nested processing
                _kwargs["_parent_converters"] = self._converters

                # Add legacy kwargs
                if stream_info is not None:
                    if stream_info.extension is not None:
                        _kwargs["file_extension"] = stream_info.extension

                    if stream_info.url is not None:
                        _kwargs["url"] = stream_info.url

                # Check if the converter will accept the file, and if so, try to convert it
                _accepts = False
                try:
                    _accepts = converter.accepts(file_stream, stream_info, **_kwargs)
                except NotImplementedError:
                    pass

                # accept() should not have changed the file stream position
                assert (
                    cur_pos == file_stream.tell()
                ), f"{type(converter).__name__}.accept() should NOT change the file_stream position"

                # Attempt the conversion
                if _accepts:
                    try:
                        res = converter.convert(file_stream, stream_info, **_kwargs)
                    except Exception:
                        failed_attempts.append(
                            FailedConversionAttempt(
                                converter=converter, exc_info=sys.exc_info()
                            )
                        )
                    finally:
                        file_stream.seek(cur_pos)

                if res is not None:
                    # Normalize the content
                    res.text_content = "\n".join(
                        [line.rstrip() for line in re.split(r"\r?\n", res.text_content)]
                    )
                    res.text_content = re.sub(r"\n{3,}", "\n\n", res.text_content)
                    return res

        # If we got this far without success, report any exceptions
        if len(failed_attempts) > 0:
            raise FileConversionException(attempts=failed_attempts)

        # Nothing can handle it!
        raise UnsupportedFormatException(
            "Could not convert stream to Markdown. No converter attempted a conversion, suggesting that the filetype is simply not supported."
        )
