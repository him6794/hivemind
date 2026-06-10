TEXT = "hivemind distributed compute sample"


def main():
    words = TEXT.split()
    print(f"word_count={len(words)}")
    print(f"uppercase={TEXT.upper()}")


if __name__ == "__main__":
    main()
