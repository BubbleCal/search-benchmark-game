import json
import re
from typing import Iterable, Iterator, List, Optional, Tuple

import lance
import pyarrow as pa

TOKEN_RE = re.compile(r'(?P<prefix>[+-]?)(?:"(?P<phrase>[^"]+)"|(?P<term>\S+))')


def iter_docs(source: Iterable) -> Iterator[dict]:
    for item in source:
        if isinstance(item, str):
            line = item.strip()
            if not line:
                continue
            try:
                doc = json.loads(line)
            except ValueError:
                continue
        elif isinstance(item, dict):
            doc = item
        else:
            continue

        doc_id = doc.get("id")
        text = doc.get("text")
        if doc_id is None or text is None:
            continue
        yield {"id": str(doc_id), "text": str(text)}


def chunked(iterable: Iterable[dict], size: int) -> Iterator[List[dict]]:
    batch: List[dict] = []
    for item in iterable:
        batch.append(item)
        if len(batch) >= size:
            yield batch
            batch = []
    if batch:
        yield batch


def build_index(docs: Iterable, idx_path: str, chunk_size: int = 10000) -> None:
    dataset = None
    for batch in chunked(iter_docs(docs), chunk_size):
        table = pa.Table.from_pylist(batch)
        if dataset is None:
            dataset = lance.write_dataset(table, idx_path, mode="overwrite")
        else:
            dataset.insert(table)

    if dataset is None:
        raise ValueError("No documents provided for indexing")

    dataset.create_scalar_index(
        "text",
        "INVERTED",
        replace=True,
        with_position=True,
        base_tokenizer="simple",
        lower_case=True,
        stem=False,
        remove_stop_words=False,
        ascii_folding=False,
    )


def sanitize_query(query: str) -> str:
    parts: List[str] = []
    for match in TOKEN_RE.finditer(query):
        phrase = match.group("phrase")
        term = match.group("term")
        text = phrase if phrase is not None else term
        if not text:
            continue
        if phrase is not None:
            parts.append(f'"{text}"')
        else:
            parts.append(text)
    return " ".join(parts)


def open_dataset(idx_path: str) -> lance.LanceDataset:
    return lance.dataset(idx_path)


def _scanner(dataset: lance.LanceDataset, query: str, limit: Optional[int]) -> lance.LanceScanner:
    if not query:
        return dataset.scanner(limit=0)
    return dataset.scanner(
        columns=[],
        full_text_query=query,
        limit=limit,
        use_scalar_index=True,
    )


def count_query(dataset: lance.LanceDataset, query: str) -> int:
    sanitized = sanitize_query(query)
    scanner = _scanner(dataset, sanitized, None)
    count = 0
    for batch in scanner.to_batches():
        count += batch.num_rows
    return count


def run_topk(dataset: lance.LanceDataset, query: str, k: int) -> None:
    sanitized = sanitize_query(query)
    scanner = _scanner(dataset, sanitized, k)
    for _ in scanner.to_batches():
        pass


def topk_count(dataset: lance.LanceDataset, query: str, k: int) -> int:
    sanitized = sanitize_query(query)
    scanner = _scanner(dataset, sanitized, None)
    count = 0
    seen = 0
    for batch in scanner.to_batches():
        if seen < k:
            seen += min(batch.num_rows, k - seen)
        count += batch.num_rows
    return count


def parse_command(command: str) -> Tuple[str, Optional[int]]:
    if command == "COUNT":
        return "count", None
    if command.startswith("TOP_") and command.endswith("_COUNT"):
        k = int(command[len("TOP_") : -len("_COUNT")])
        return "topk_count", k
    if command.startswith("TOP_"):
        k = int(command[len("TOP_") :])
        return "topk", k
    return "unsupported", None
