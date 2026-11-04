# markitdown's built-in converters, each backed by the markitdown-rs
# converter of the same name. Features markitdown implements in Python on
# top of the conversion (LLM image captions, audio transcription, YouTube
# transcripts) follow the original converters line by line.
import io
import re
import time
from typing import Any, BinaryIO, Optional, Union
from urllib.parse import parse_qs, urlparse

from .. import _markitdown_rs as _native
from .._base_converter import DocumentConverter, DocumentConverterResult
from .._stream_info import StreamInfo
from .._exceptions import MissingDependencyException
from ._llm_caption import llm_caption
from ._transcribe_audio import transcribe_audio


def _fields(stream_info: StreamInfo):
    return (
        stream_info.mimetype,
        stream_info.extension,
        stream_info.charset,
        stream_info.filename,
        stream_info.local_path,
        stream_info.url,
    )


class _SharedBytes(io.BytesIO):
    """The input stream MarkItDown hands to converters; native converters
    read its bytes without copying them."""

    def __init__(self, data: bytes):
        super().__init__(data)
        self.data = data


def _read(file_stream: BinaryIO) -> bytes:
    if isinstance(file_stream, _SharedBytes) and file_stream.tell() == 0:
        return file_stream.data
    pos = file_stream.tell()
    try:
        return file_stream.read()
    finally:
        file_stream.seek(pos)


class NativeConversionError(Exception):
    """A markitdown-rs converter failed on the input."""


class _NativeConverter(DocumentConverter):
    _native_name = ""

    def accepts(
        self,
        file_stream: BinaryIO,
        stream_info: StreamInfo,
        **kwargs: Any,
    ) -> bool:
        return _native.accepts(
            self._native_name, _read(file_stream), _fields(stream_info)
        )

    def _image_hook(self, kwargs: dict):
        return None

    def _native_markdown(
        self, data: bytes, stream_info: StreamInfo, **kwargs: Any
    ) -> str:
        status, text = _native.convert_with(
            self._native_name,
            data,
            _fields(stream_info),
            bool(kwargs.get("keep_data_uris", False)),
            # markitdown looks exiftool up in MarkItDown, not in the converters
            kwargs.get("exiftool_path") or "",
            self._image_hook(kwargs),
        )
        if status != "ok":
            raise NativeConversionError(text)
        return text

    def convert(
        self,
        file_stream: BinaryIO,
        stream_info: StreamInfo,
        **kwargs: Any,
    ) -> DocumentConverterResult:
        markdown = self._native_markdown(_read(file_stream), stream_info, **kwargs)
        return DocumentConverterResult(markdown=markdown)


def _native_converter(name: str, doc: str) -> type:
    return type(name, (_NativeConverter,), {"_native_name": name, "__doc__": doc})


PlainTextConverter = _native_converter(
    "PlainTextConverter", "Anything with content type text/plain"
)
RssConverter = _native_converter("RssConverter", "Convert RSS / Atom type to markdown")
WikipediaConverter = _native_converter(
    "WikipediaConverter", "Handle Wikipedia pages separately, focusing only on the main document content."
)
IpynbConverter = _native_converter(
    "IpynbConverter", "Converts Jupyter Notebook (.ipynb) files to Markdown."
)
BingSerpConverter = _native_converter(
    "BingSerpConverter", "Handle Bing results pages (only the organic search results)."
)
PdfConverter = _native_converter("PdfConverter", "Converts PDFs to Markdown.")
XlsConverter = _native_converter(
    "XlsConverter", "Converts XLS files to Markdown, with each sheet presented as a separate Markdown table."
)
OutlookMsgConverter = _native_converter(
    "OutlookMsgConverter", "Converts Outlook .msg files to markdown."
)
ZipConverter = _native_converter(
    "ZipConverter", "Converts ZIP files to markdown by converting each file it contains."
)
EpubConverter = _native_converter("EpubConverter", "Converts EPUB files to Markdown.")
CsvConverter = _native_converter("CsvConverter", "Converts CSV files to Markdown tables.")


def _check_image_html(fragment: Optional[str]) -> Optional[str]:
    """markitdown's `_parse_image_html`: a fragment, None or blank."""
    if fragment is not None and not isinstance(fragment, str):
        raise TypeError("_image_to_html must return an HTML string or None")
    if fragment is None or not fragment.strip():
        return None
    if re.search(r"<\s*(html|head|body)[\s/>]|<!doctype", fragment, re.IGNORECASE):
        raise ValueError("_image_to_html must return a fragment, not a document")
    return fragment


class _OfficeConverter(_NativeConverter):
    """A converter with markitdown's `_image_to_html` hook for embedded
    images (and, for presentations, LLM captions)."""

    _llm_captions = False

    def _image_hook(self, kwargs: dict):
        custom = type(self)._image_to_html is not type(self)._own_image_to_html()
        llm_client = kwargs.get("llm_client")
        llm_model = kwargs.get("llm_model")
        captions = self._llm_captions and llm_client is not None and llm_model is not None
        if not (custom or captions):
            return None

        def hook(request, data, mimetype, extension, filename):
            stream_info = StreamInfo(
                mimetype=mimetype, extension=extension, filename=filename
            )
            if request == "caption":
                if not captions:
                    return None
                with io.BytesIO(data) as image_stream:
                    try:
                        return llm_caption(
                            image_stream,
                            stream_info,
                            client=llm_client,
                            model=llm_model,
                            prompt=kwargs.get("llm_prompt"),
                        )
                    except Exception:
                        # Preserve native caption failure fallback.
                        return None
            if not custom:
                return None
            with io.BytesIO(data) as image_stream:
                fragment = self._image_to_html(image_stream, stream_info, **kwargs)
            return _check_image_html(fragment)

        return hook

    @classmethod
    def _own_image_to_html(cls):
        for klass in cls.__mro__:
            if klass in (DocxConverter, PptxConverter, XlsxConverter):
                return klass._image_to_html
        return None


class DocxConverter(_OfficeConverter):
    """
    Converts DOCX files to Markdown. Style information (e.g., headings) and tables are preserved where possible.
    """

    _native_name = "DocxConverter"

    def convert(
        self,
        file_stream: BinaryIO,
        stream_info: StreamInfo,
        **kwargs: Any,  # Options to pass to the converter
    ) -> DocumentConverterResult:
        if kwargs.get("style_map"):
            raise NotImplementedError(
                "markitdown-rs does not support custom mammoth style maps (style_map)"
            )
        return super().convert(file_stream, stream_info, **kwargs)

    def _image_to_html(
        self,
        image_stream: BinaryIO,
        stream_info: StreamInfo,
        **kwargs: Any,
    ) -> Optional[str]:
        """Override to render an embedded image as an HTML fragment.

        The stream is borrowed, seekable, and positioned at zero; do not close
        or retain it. StreamInfo describes the image, not the document.
        Existing conversion options are forwarded through kwargs.

        Return None or blank text to retain the native image representation.
        Otherwise return HTML, escaping any literal text. Inline HTML stays
        inline; block HTML splits enclosing paragraph/inline wrappers but stays
        inside its table cell or list item. Hook failures propagate through the
        normal conversion failure path.
        """
        return None


class PptxConverter(_OfficeConverter):
    """
    Converts PPTX files to Markdown. Supports heading, tables and images with alt text.
    """

    _native_name = "PptxConverter"
    _llm_captions = True

    def _image_to_html(
        self,
        image_stream: BinaryIO,
        stream_info: StreamInfo,
        **kwargs: Any,
    ) -> Optional[str]:
        """Override to render an embedded image as an HTML fragment.

        The stream is borrowed, seekable, and positioned at zero; do not close
        or retain it. StreamInfo describes the image, not the presentation.
        Existing conversion options are forwarded through kwargs.

        Native LLM captions take precedence. Otherwise, return None or blank
        text to retain the native image representation, or HTML with literal
        text escaped. The fragment passes through HtmlConverter before being
        placed at the picture's position in slide/group order. Hook failures
        propagate through the normal conversion failure path.
        """
        return None


class XlsxConverter(_OfficeConverter):
    """
    Converts XLSX files to Markdown, with each sheet presented as a separate Markdown table.
    """

    _native_name = "XlsxConverter"

    def _image_to_html(
        self,
        image_stream: BinaryIO,
        stream_info: StreamInfo,
        **kwargs: Any,
    ) -> Optional[str]:
        """Override to render an embedded image as an HTML fragment.

        The stream is borrowed, seekable, and positioned at zero; do not close
        or retain it. StreamInfo describes the image, not the workbook. Existing
        conversion options are forwarded through kwargs.

        Return None or blank text to retain the native representation (no image
        output). Otherwise return HTML, escaping any literal text. Images appear
        after their sheet's table, in the existing worksheet image order. Linked
        images are not fetched. Hook failures propagate through the normal
        conversion failure path.
        """
        return None


class HtmlConverter(_NativeConverter):
    """Anything with content type text/html"""

    _native_name = "HtmlConverter"

    def convert_string(
        self, html_content: str, *, url: Optional[str] = None, **kwargs
    ) -> DocumentConverterResult:
        """
        Non-standard convenience method to convert a string to markdown.
        Given that many converters produce HTML as intermediate output, this
        allows for easy conversion of HTML to markdown.
        """
        return self.convert(
            file_stream=io.BytesIO(html_content.encode("utf-8")),
            stream_info=StreamInfo(
                mimetype="text/html",
                extension=".html",
                charset="utf-8",
                url=url,
            ),
            **kwargs,
        )


class ImageConverter(_NativeConverter):
    """
    Converts images to markdown via extraction of metadata (if `exiftool` is installed), and description via a multimodal LLM (if an llm_client is configured).
    """

    _native_name = "ImageConverter"

    def convert(
        self,
        file_stream: BinaryIO,
        stream_info: StreamInfo,
        **kwargs: Any,  # Options to pass to the converter
    ) -> DocumentConverterResult:
        md_content = self._native_markdown(_read(file_stream), stream_info, **kwargs)

        # Try describing the image with GPT
        llm_client = kwargs.get("llm_client")
        llm_model = kwargs.get("llm_model")
        if llm_client is not None and llm_model is not None:
            llm_description = llm_caption(
                file_stream,
                stream_info,
                client=llm_client,
                model=llm_model,
                prompt=kwargs.get("llm_prompt"),
            )
            if llm_description is not None:
                md_content += "\n# Description:\n" + llm_description.strip() + "\n"

        return DocumentConverterResult(
            markdown=md_content,
        )


class AudioConverter(_NativeConverter):
    """
    Converts audio files to markdown via extraction of metadata (if `exiftool` is installed), and speech transcription (if `speech_recognition` is installed).
    """

    _native_name = "AudioConverter"

    def convert(
        self,
        file_stream: BinaryIO,
        stream_info: StreamInfo,
        **kwargs: Any,  # Options to pass to the converter
    ) -> DocumentConverterResult:
        md_content = self._native_markdown(_read(file_stream), stream_info, **kwargs)

        # Figure out the audio format for transcription
        if stream_info.extension == ".wav" or stream_info.mimetype == "audio/x-wav":
            audio_format = "wav"
        elif stream_info.extension == ".mp3" or stream_info.mimetype == "audio/mpeg":
            audio_format = "mp3"
        elif (
            stream_info.extension in [".mp4", ".m4a"]
            or stream_info.mimetype == "video/mp4"
        ):
            audio_format = "mp4"
        else:
            audio_format = None

        # Figure out the audio language for transcription, default to "en-US"
        language = kwargs.get("language", "en-US")

        # Transcribe
        if audio_format:
            try:
                transcript = transcribe_audio(
                    file_stream, audio_format=audio_format, language=language
                )
                if transcript:
                    md_content += "\n\n### Audio Transcript:\n" + transcript
            except MissingDependencyException:
                pass

        # Return the result
        return DocumentConverterResult(markdown=md_content.strip())


# Optional YouTube transcription support
try:
    # Suppress some warnings on library import
    import warnings

    with warnings.catch_warnings():
        warnings.filterwarnings("ignore", category=SyntaxWarning)
        # Patch submitted upstream to fix the SyntaxWarning
        from youtube_transcript_api import YouTubeTranscriptApi

    IS_YOUTUBE_TRANSCRIPT_CAPABLE = True
except ModuleNotFoundError:
    IS_YOUTUBE_TRANSCRIPT_CAPABLE = False


class YouTubeConverter(_NativeConverter):
    """Extract YouTube metadata and transcripts, or fall back to HTML."""

    _native_name = "YouTubeConverter"

    def _get_video_id(self, url: str) -> Union[str, None]:
        """Extract a YouTube video ID from supported URL formats."""
        parsed_url = urlparse(url)
        hostname = parsed_url.netloc.lower()
        path_parts = [part for part in parsed_url.path.split("/") if part]

        if hostname in {"youtu.be", "www.youtu.be"}:
            return path_parts[0] if path_parts else None

        if hostname in {"youtube.com", "www.youtube.com", "m.youtube.com"}:
            if path_parts[:1] == ["watch"]:
                params = parse_qs(parsed_url.query)
                return params.get("v", [None])[0]
            if path_parts[:1] in (["shorts"], ["embed"]):
                return path_parts[1] if len(path_parts) > 1 else None

        return None

    def convert(
        self,
        file_stream: BinaryIO,
        stream_info: StreamInfo,
        **kwargs: Any,  # Options to pass to the converter
    ) -> DocumentConverterResult:
        data = _read(file_stream)
        markdown = self._native_markdown(data, stream_info, **kwargs)
        # markitdown-rs gives "# YouTube\n" plus the metadata sections, or
        # the page as HTML when the page has none
        has_metadata = markdown.startswith("# YouTube\n")
        webpage_text = markdown[len("# YouTube\n") :] if has_metadata else ""

        if IS_YOUTUBE_TRANSCRIPT_CAPABLE:
            ytt_api = YouTubeTranscriptApi()
            transcript_text = ""
            video_id = self._get_video_id(stream_info.url or "")
            if video_id:
                transcript_list = ytt_api.list(video_id)
                languages = ["en"]
                for transcript in transcript_list:
                    languages.append(transcript.language_code)
                    break
                try:
                    youtube_transcript_languages = kwargs.get(
                        "youtube_transcript_languages", languages
                    )
                    # Retry the transcript fetching operation
                    transcript = self._retry_operation(
                        lambda: ytt_api.fetch(
                            video_id, languages=youtube_transcript_languages
                        ),
                        retries=3,  # Retry 3 times
                        delay=2,  # 2 seconds delay between retries
                    )

                    if transcript:
                        transcript_text = " ".join(
                            [part.text for part in transcript]
                        )  # type: ignore
                except Exception as e:
                    # No transcript available
                    if len(languages) == 1:
                        print(f"Error fetching transcript: {e}")
                    else:
                        # Translate transcript into first kwarg
                        transcript = (
                            transcript_list.find_transcript(languages)
                            .translate(youtube_transcript_languages[0])
                            .fetch()
                        )
                        transcript_text = " ".join([part.text for part in transcript])
            if transcript_text:
                webpage_text += f"\n### Transcript\n{transcript_text}\n"

        if not webpage_text:
            return DocumentConverterResult(markdown=markdown)

        return DocumentConverterResult(markdown="# YouTube\n" + webpage_text)

    def _retry_operation(self, operation, retries=3, delay=2):
        """Retries the operation if it fails."""
        attempt = 0
        while attempt < retries:
            try:
                return operation()  # Attempt the operation
            except Exception as e:
                print(f"Attempt {attempt + 1} failed: {e}")
                if attempt < retries - 1:
                    time.sleep(delay)  # Wait before retrying
                attempt += 1
        # If all attempts fail, raise the last exception
        raise Exception(f"Operation failed after {retries} attempts.")
