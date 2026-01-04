import argparse
import csv
import json
from pathlib import Path
from typing import Dict, Iterable, List, Optional, Sequence, Tuple


def _percentile(sorted_values: Sequence[int], p: float) -> int:
    if not sorted_values:
        raise ValueError("percentile requires at least one value")
    idx = int((len(sorted_values) - 1) * p + 0.5)
    return sorted_values[idx]


def _query_stat(durations: Sequence[int], stat: str) -> Optional[float]:
    if not durations:
        return None
    values = sorted(durations)
    if stat == "median" or stat == "p50":
        return float(_percentile(values, 0.5))
    if stat == "mean":
        return sum(values) / len(values)
    if stat == "min":
        return float(values[0])
    if stat == "max":
        return float(values[-1])
    if stat == "p90":
        return float(_percentile(values, 0.9))
    if stat == "p99":
        return float(_percentile(values, 0.99))
    raise ValueError(f"Unknown query stat: {stat}")


def _aggregate(values: List[float]) -> Dict[str, int]:
    values_sorted = sorted(values)
    avg = int(round(sum(values_sorted) / len(values_sorted)))
    p50 = _percentile(values_sorted, 0.5)
    p90 = _percentile(values_sorted, 0.9)
    p99 = _percentile(values_sorted, 0.99)
    max_val = int(values_sorted[-1])
    return {
        "avg_us": avg,
        "p50_us": int(p50),
        "p90_us": int(p90),
        "p99_us": int(p99),
        "max_us": max_val,
    }


def build_rows(
    data: Dict,
    query_stat: str = "median",
    engines: Optional[Sequence[str]] = None,
    include_all: bool = True,
) -> List[Dict[str, object]]:
    results = data.get("results", {})
    rows: List[Dict[str, object]] = []

    for command, command_results in results.items():
        engine_names = list(command_results.keys())
        if engines is not None:
            engine_names = [e for e in engine_names if e in engines]

        for engine in engine_names:
            engine_queries = command_results[engine]
            tag_values: Dict[str, List[float]] = {}

            for query in engine_queries:
                value = _query_stat(query.get("duration", []), query_stat)
                if value is None:
                    continue
                tags = query.get("tags", [])
                for tag in tags:
                    tag_values.setdefault(tag, []).append(value)
                if include_all:
                    tag_values.setdefault("ALL", []).append(value)

            for tag, values in sorted(tag_values.items()):
                if not values:
                    continue
                stats = _aggregate(values)
                rows.append(
                    {
                        "command": command,
                        "engine": engine,
                        "tag": tag,
                        "query_stat": query_stat,
                        "num_queries": len(values),
                        **stats,
                    }
                )

    return rows


def write_csv(rows: Iterable[Dict[str, object]], output_path: Path) -> None:
    fieldnames = [
        "command",
        "engine",
        "tag",
        "query_stat",
        "num_queries",
        "avg_us",
        "p50_us",
        "p90_us",
        "p99_us",
        "max_us",
    ]
    with output_path.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            writer.writerow(row)


def main() -> None:
    parser = argparse.ArgumentParser(
        description=(
            "Aggregate benchmark results by query tag and write summary stats to CSV."
        )
    )
    parser.add_argument(
        "--input",
        default="results.json",
        help="Path to results.json (default: results.json)",
    )
    parser.add_argument(
        "--output",
        default="results_by_tag.csv",
        help="Output CSV path (default: results_by_tag.csv)",
    )
    parser.add_argument(
        "--query-stat",
        default="median",
        choices=["median", "p50", "mean", "min", "max", "p90", "p99"],
        help="How to summarize per-query latency before aggregation (default: median)",
    )
    parser.add_argument(
        "--engines",
        nargs="*",
        help="Optional list of engines to include",
    )
    parser.add_argument(
        "--no-include-all",
        action="store_true",
        help="Exclude the ALL tag aggregation",
    )
    args = parser.parse_args()

    input_path = Path(args.input)
    output_path = Path(args.output)

    with input_path.open("r", encoding="utf-8") as f:
        data = json.load(f)

    rows = build_rows(
        data,
        query_stat=args.query_stat,
        engines=args.engines,
        include_all=not args.no_include_all,
    )
    write_csv(rows, output_path)


if __name__ == "__main__":
    main()
