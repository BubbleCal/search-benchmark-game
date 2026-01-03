import sys

from pylance_engine import count_query, open_dataset, parse_command, run_topk, topk_count


def main() -> None:
    idx_path = sys.argv[1] if len(sys.argv) > 1 else "idx"
    dataset = open_dataset(idx_path)

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
            result = count_query(dataset, query)
            print(result)
        elif action == "topk":
            run_topk(dataset, query, k or 0)
            print(1)
        elif action == "topk_count":
            result = topk_count(dataset, query, k or 0)
            print(result)
        else:
            print("UNSUPPORTED")

        sys.stdout.flush()


if __name__ == "__main__":
    main()
