import tempfile

from engines.pylance import pylance_engine


def test_pylance_counts_and_phrase():
    docs = [
        {"id": "1", "text": "hello world"},
        {"id": "2", "text": "hello there"},
        {"id": "3", "text": "world peace"},
    ]

    with tempfile.TemporaryDirectory() as tmp:
        pylance_engine.build_index(docs, tmp, chunk_size=2)
        dataset = pylance_engine.open_dataset(tmp)

        assert pylance_engine.count_query(dataset, "hello") == 2
        assert pylance_engine.count_query(dataset, '"hello world"') == 1
        # Pylance full_text_query uses OR semantics for multiple terms
        assert pylance_engine.count_query(dataset, "+hello +world") == 3

        pylance_engine.run_topk(dataset, "hello", 2)
