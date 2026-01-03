import tempfile

from engines.lancedb import lancedb_engine


def test_lancedb_counts_and_phrase():
    docs = [
        {"id": "1", "text": "hello world"},
        {"id": "2", "text": "hello there"},
        {"id": "3", "text": "world peace"},
    ]

    with tempfile.TemporaryDirectory() as tmp:
        lancedb_engine.build_index(docs, tmp, chunk_size=2)
        table = lancedb_engine.open_table(tmp)

        assert lancedb_engine.count_query(table, "hello") == 2
        assert lancedb_engine.count_query(table, '"hello world"') == 1
        assert lancedb_engine.count_query(table, "+hello +world") == 1

        lancedb_engine.run_topk(table, "hello", 2)
