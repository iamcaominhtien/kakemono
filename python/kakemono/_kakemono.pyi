from typing import List, Optional

__version__: str

def convert(
    pdf: bytes,
    modes: List[str],
    *,
    source: Optional[str] = None,
    tables: bool = True,
    equations: str = "mathml",
    page_image: str = "svg",
    embed_fonts: bool = True,
    scale: float = 3.0,
    jobs: int = 0,
) -> List[str]: ...
def analyze(pdf: bytes, *, tables: bool = True, jobs: int = 0) -> str: ...
def parse_arxiv_id(text: str) -> Optional[str]: ...
def fetch_arxiv(id: str) -> bytes: ...
