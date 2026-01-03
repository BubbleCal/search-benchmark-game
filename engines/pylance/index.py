import sys

from pylance_engine import build_index


def main() -> None:
    idx_path = sys.argv[1] if len(sys.argv) > 1 else "idx"
    build_index(sys.stdin, idx_path)


if __name__ == "__main__":
    main()
