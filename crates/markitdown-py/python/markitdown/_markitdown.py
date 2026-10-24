# The MarkItDown class of microsoft/markitdown 0.1.9 (MIT), with the
# converters replaced by markitdown-rs. Source handling (paths, URIs,
# streams, requests responses) follows the original line by line.
import io
import os
from email.message import Message
from email.utils import collapse_rfc2231_value
from pathlib import Path
from typing import Any, BinaryIO, Optional, Union
from urllib.parse import urlparse

from . import _markitdown_rs as _native
from ._base_converter import DocumentConverterResult
from ._exceptions import FileConversionException, UnsupportedFormatException
from ._stream_info import StreamInfo

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


class MarkItDown:
    """Convert files, streams and URIs to Markdown, as markitdown does.

    Plugins, LLM image captions and Azure Document Intelligence / Content
    Understanding are not available in markitdown-rs.
    """

    def __init__(
        self,
        *,
        enable_builtins: Union[None, bool] = None,
        enable_plugins: Union[None, bool] = None,
        **kwargs: Any,
    ):
        if enable_plugins:
            raise NotImplementedError("markitdown-rs does not support plugins")
        for key in ("docintel_endpoint", "cu_endpoint"):
            if kwargs.get(key):
                raise NotImplementedError(f"markitdown-rs does not support {key}")

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

    def register_converter(self, *args: Any, **kwargs: Any) -> None:
        raise NotImplementedError("markitdown-rs does not support custom converters")

    def _convert(
        self, data: bytes, info: StreamInfo, **kwargs: Any
    ) -> DocumentConverterResult:
        fields = (
            info.mimetype,
            info.extension,
            info.charset,
            info.filename,
            info.local_path,
            info.url,
        )
        status, text = _native.convert(
            data, fields, bool(kwargs.get("keep_data_uris", False))
        )
        if status == "ok":
            return DocumentConverterResult(markdown=text)
        if status == "unsupported":
            raise UnsupportedFormatException(
                "Could not convert stream to Markdown. No converter attempted a conversion, suggesting that the filetype is simply not supported."
            )
        raise FileConversionException(text)
