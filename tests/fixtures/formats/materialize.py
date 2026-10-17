"""Expand the committed synthetic specifications into an empty loose corpus."""
import argparse
from pathlib import Path


def expand(path: Path) -> bytearray:
    result = bytearray()
    for line in path.read_text().splitlines():
        address, payload = line.split(":")
        span = address.split("-")
        assert int(span[0], 16) == len(result), "fixture has an implicit gap"
        data = bytes.fromhex(payload)
        if len(span) == 2:
            assert len(data) == 1
            data *= int(span[1], 16) - len(result) + 1
        result.extend(data)
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    output = parser.parse_args().output
    if output.exists() and any(output.iterdir()):
        parser.error("--output must be an empty directory")
    source = Path(__file__).resolve().parent
    palette = expand(source / "palette.hex")
    # SPEC-002 section 4.2's fully specified grayscale subtest.
    for index in range(32):
        palette[index * 3:index * 3 + 3] = bytes([index * 2]) * 3
        palette[768 + index * 1024 + index * 32 + index] = index
    files = {
        "color.pal": palette,
        "font0.fon": expand(source / "font.fon.hex"),
        "font0.aaf": expand(source / "font.aaf.hex"),
        "art/items/shared.frm": expand(source / "shared.frm.hex"),
        "art/items/animated.frm": expand(source / "animated.frm.hex"),
        "art/items/items.lst": (source / "art.lst").read_bytes(),
        "proto/items/items.lst": (source / "proto.lst").read_bytes(),
        "scripts/scripts.lst": (source / "scripts.lst").read_bytes(),
        "text/english/game/sample.msg": (source / "messages.msg").read_bytes(),
        "text/english/game/preview.msg": b"{1}{}{AB A B}",
    }
    for direction in range(6):
        files[f"art/items/shared.fr{direction}"] = expand(source / "shared.frm.hex")
    for name, data in files.items():
        destination = output / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
    print(f"Wrote {len(files)} synthetic assets to {output}")


if __name__ == "__main__":
    main()
