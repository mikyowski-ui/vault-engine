#!/usr/bin/env python3
"""Expand the committed synthetic fixtures into starting corpora.

Run from the repository root:  python3 -I fuzz/seed.py
Writes fuzz/corpus/<target>/ (gitignored). Every seed is a synthetic fixture
authored for Vault Engine; never add real game assets here.
"""
import pathlib
import struct
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "tests" / "fixtures"
CORPUS = ROOT / "fuzz" / "corpus"


def plain(text):
    """Space-separated hex bytes over one or more lines."""
    return bytes(int(byte, 16) for byte in text.split())


def sparse(text):
    """`ADDR: bytes` lines; `START-END: byte` repeats one byte over a range."""
    out = bytearray()
    for line in filter(None, text.splitlines()):
        address, payload = line.split(":", 1)
        start, _, end = address.partition("-")
        if int(start, 16) != len(out):
            raise ValueError(f"fixture skips to {start}")
        payload = plain(payload)
        if end:
            out.extend(payload * (int(end, 16) + 1 - len(out)))
        else:
            out.extend(payload)
    return bytes(out)


def read(path):
    text = path.read_text()
    return sparse(text) if ":" in text else plain(text)


def packed_members(archive):
    """(logical length, packed body) of each `0x40` member, from the tables."""
    word = lambda at: struct.unpack_from(">I", archive, at)[0]
    count, _, data_size, _ = struct.unpack_from(">4I", archive, 0)
    at, names = 16, 0
    for _ in range(count):
        at += 1 + archive[at] + data_size
        names += 1
    members = []
    for _ in range(names):
        files, _, data_size, _ = struct.unpack_from(">4I", archive, at)
        at += 16
        for _ in range(files):
            at += 1 + archive[at]
            attributes, offset, length, packed = (word(at + 4 * k) for k in range(4))
            at += data_size
            if attributes & 0xF0 == 0x40:
                members.append((length, archive[offset : offset + packed]))
    return members


def write(target, name, data):
    directory = CORPUS / target
    directory.mkdir(parents=True, exist_ok=True)
    (directory / name).write_bytes(data)


def main():
    dat = read(FIXTURES / "archive" / "synthetic.dat.hex")
    write("dat", "synthetic.dat", dat)
    for index, (length, body) in enumerate(packed_members(dat)):
        write("lzss", f"member-{index}", struct.pack("<I", length) + body)
    for path in sorted((FIXTURES / "formats").glob("*.frm.hex")):
        write("frm", path.name.removesuffix(".hex"), read(path))
    for path in sorted((FIXTURES / "formats").glob("palette*.hex")):
        write("pal", path.name.removesuffix(".hex") + ".pal", read(path))
    for path in sorted((FIXTURES / "sound").glob("*.acm.hex")):
        write("acm", path.name.removesuffix(".hex"), read(path))
    header = read(FIXTURES / "movie" / "fixture-a.hex")
    write("mve", "fixture-a.mve", header)
    write("mve", "fixture-b.mve", header[:26] + read(FIXTURES / "movie" / "fixture-b.hex"))
    for path in sorted((ROOT / "oracle" / "SPEC-012" / "synthetic").glob("*.mve")):
        write("mve", path.name, path.read_bytes())
    for path in sorted((FIXTURES / "world").glob("*.map.hex")):
        write("map", path.name.removesuffix(".hex"), read(path))
    for directory in sorted(CORPUS.iterdir()):
        print(f"{directory.relative_to(ROOT)}: {len(list(directory.iterdir()))} seeds")


if __name__ == "__main__":
    sys.exit(main())
