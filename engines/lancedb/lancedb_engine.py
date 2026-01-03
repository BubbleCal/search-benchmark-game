import json
import re
from typing import Iterable, Iterator, List, Optional, Tuple

import lancedb
from lancedb.query import BooleanQuery, FullTextOperator, MatchQuery, Occur

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
    db = lancedb.connect(idx_path)
    table = None
    for batch in chunked(iter_docs(docs), chunk_size):
        if table is None:
            table = db.create_table("docs", batch, mode="overwrite")
        else:
            table.add(batch)

    if table is None:
        raise ValueError("No documents provided for indexing")

    table.create_fts_index(
        "text",
        replace=True,
        use_tantivy=False,
    )


def open_table(idx_path: str):
    db = lancedb.connect(idx_path)
    return db.open_table("docs")


def build_fts_query(query: str, column: str = "text"):
    terms: List[Tuple[Occur, object]] = []
    for match in TOKEN_RE.finditer(query):
        prefix = match.group("prefix")
        phrase = match.group("phrase")
        term = match.group("term")
        text = phrase if phrase is not None else term
        if not text:
            continue

        if phrase is not None:
            q = MatchQuery(text, column, operator=FullTextOperator.AND)
        else:
            q = MatchQuery(text, column, operator=FullTextOperator.OR)

        if prefix == "+":
            occur = Occur.MUST
        elif prefix == "-":
            occur = Occur.MUST_NOT
        else:
            occur = Occur.SHOULD
        terms.append((occur, q))

    if not terms:
        return MatchQuery("", column, operator=FullTextOperator.OR)
    if len(terms) == 1 and terms[0][0] == Occur.SHOULD:
        return terms[0][1]
    return BooleanQuery(terms)


def _query_builder(table, query: str, limit: Optional[int]):
    fts_query = build_fts_query(query)
    qb = table.search(fts_query, query_type="fts")
    return qb.limit(limit)


def _execute_reader(table, query: str, limit: Optional[int]):
    qb = _query_builder(table, query, limit)
    query_obj = qb.to_query_object()
    return table._execute_query(query_obj)


def count_query(table, query: str) -> int:
    reader = _execute_reader(table, query, None)
    count = 0
    for batch in reader:
        count += batch.num_rows
    return count


def run_topk(table, query: str, k: int) -> None:
    reader = _execute_reader(table, query, k)
    for _ in reader:
        pass


def topk_count(table, query: str, k: int) -> int:
    reader = _execute_reader(table, query, None)
    count = 0
    seen = 0
    for batch in reader:
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
