from results_to_csv import build_rows


def test_build_rows_median():
    data = {
        "results": {
            "TOP_100": {
                "lucene": [
                    {
                        "query": "q1",
                        "tags": ["term"],
                        "duration": [10, 20, 30],
                        "count": 1,
                    },
                    {
                        "query": "q2",
                        "tags": ["phrase", "global"],
                        "duration": [5, 15, 25],
                        "count": 1,
                    },
                    {
                        "query": "q3",
                        "tags": ["term", "global"],
                        "duration": [],
                        "count": 0,
                    },
                ]
            }
        }
    }

    rows = build_rows(data, query_stat="median", include_all=True)
    row_map = {(r["command"], r["engine"], r["tag"]): r for r in rows}

    term_row = row_map[("TOP_100", "lucene", "term")]
    assert term_row["num_queries"] == 1
    assert term_row["avg_us"] == 20
    assert term_row["p50_us"] == 20
    assert term_row["p90_us"] == 20
    assert term_row["p99_us"] == 20
    assert term_row["max_us"] == 20

    phrase_row = row_map[("TOP_100", "lucene", "phrase")]
    assert phrase_row["avg_us"] == 15
    assert phrase_row["max_us"] == 15

    all_row = row_map[("TOP_100", "lucene", "ALL")]
    assert all_row["num_queries"] == 2
    assert all_row["avg_us"] == 18
    assert all_row["p50_us"] == 20
    assert all_row["p90_us"] == 20
    assert all_row["p99_us"] == 20
    assert all_row["max_us"] == 20
