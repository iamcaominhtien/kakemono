from typing import List, Optional, Tuple

__version__: str

class Cancelled(Exception): ...

class CancelToken:
    def __init__(self, *, timeout: Optional[float] = None) -> None: ...
    def cancel(self) -> None: ...
    @property
    def cancelled(self) -> bool: ...
    def child(self, *, timeout: Optional[float] = None) -> CancelToken: ...

class Document:
    def __init__(
        self,
        data: Optional[bytes] = None,
        *,
        path: Optional[str] = None,
        url: Optional[str] = None,
        source: Optional[str] = None,
        tables: bool = True,
        jobs: int = 0,
        png_compression: str = "balanced",
        content: Optional[str] = None,
        content_dir: Optional[str] = None,
        header_rows: Optional[int] = None,
        cancel: Optional[CancelToken] = None,
        timeout: Optional[float] = None,
    ) -> None: ...
    @property
    def format(self) -> str: ...
    @property
    def page_count(self) -> int: ...
    def convert(
        self,
        modes: List[str],
        *,
        equations: str = "mathml",
        page_image: str = "svg",
        embed_fonts: bool = True,
        scale: float = 3.0,
        cancel: Optional[CancelToken] = None,
        timeout: Optional[float] = None,
    ) -> List[str]: ...
    def to_docx(
        self, *, equations: str = "mathml", scale: float = 3.0, image_dir: Optional[str] = None, cancel: Optional[CancelToken] = None, timeout: Optional[float] = None
    ) -> Tuple[bytes, List[Tuple[str, bytes]]]: ...
    def to_markdown(
        self,
        *,
        equations: str = "mathml",
        scale: float = 3.0,
        image_dir: str = "images",
        inline_images: bool = False,
        mode: str = "friendly",
        cancel: Optional[CancelToken] = None,
        timeout: Optional[float] = None,
    ) -> Tuple[str, List[Tuple[str, bytes]]]: ...
    def to_json(
        self, *, equations: str = "mathml", scale: float = 3.0, image_dir: str = "images", inline_images: bool = False, cancel: Optional[CancelToken] = None, timeout: Optional[float] = None
    ) -> Tuple[str, List[Tuple[str, bytes]]]: ...
    def to_latex(
        self, *, equations: str = "mathml", scale: float = 3.0, image_dir: str = "images", cancel: Optional[CancelToken] = None, timeout: Optional[float] = None
    ) -> Tuple[str, List[Tuple[str, bytes]]]: ...
    def to_typst(
        self, *, equations: str = "mathml", scale: float = 3.0, image_dir: str = "images", layout: str = "auto", cancel: Optional[CancelToken] = None, timeout: Optional[float] = None
    ) -> Tuple[str, List[Tuple[str, bytes]]]: ...
    def to_epub(
        self,
        *,
        equations: str = "mathml",
        scale: float = 3.0,
        max_math_width_em: float = 20.0,
        cover: bool = True,
        modified: Optional[str] = None,
        cancel: Optional[CancelToken] = None,
        timeout: Optional[float] = None,
    ) -> bytes: ...
    def to_chunks(
        self, *, max_tokens: int = 512, tokenizer: Optional[str] = None, cancel: Optional[CancelToken] = None, timeout: Optional[float] = None
    ) -> str: ...
    def to_pdf(self, *, cancel: Optional[CancelToken] = None, timeout: Optional[float] = None) -> bytes: ...
    def analyze(self, *, cancel: Optional[CancelToken] = None, timeout: Optional[float] = None) -> str: ...

def render_svg(svg: str, *, scale: float = 1.0, resources_dir: Optional[str] = None) -> bytes: ...
def read_formula(image: bytes) -> Optional[str]: ...
def parse_arxiv_id(text: str) -> Optional[str]: ...
def fetch_arxiv(id: str, *, cancel: Optional[CancelToken] = None, timeout: Optional[float] = None) -> bytes: ...
