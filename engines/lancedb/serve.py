import sys

from lancedb_engine import count_query, open_table, parse_command, run_topk, topk_count


def main() -> None:
    idx_path = sys.argv[1] if len(sys.argv) > 1 else "idx"
    table = open_table(idx_path)

    for line in sys.stdin:
        line = line.strip("\n")
        if not line:
            continue
        if "\t" not in line:
            print("UNSUPPORTED")
            sys.stdout.flush()
            continue

        command, query = line.split("\t", 1)
        action, k = parse_command(command)

        if action == "count":
            result = count_query(table, query)
            print(result)
        elif action == "topk":
            run_topk(table, query, k or 0)
            print(1)
        elif action == "topk_count":
            result = topk_count(table, query, k or 0)
            print(result)
        else:
            print("UNSUPPORTED")

        sys.stdout.flush()


if __name__ == "__main__":
    main()
