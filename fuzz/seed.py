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


def read_text(path):
    return path.read_text()


def dat2(members):
    """A SPEC-015 archive: member data, the tree, then the footer."""
    data = bytearray()
    tree = bytearray(struct.pack("<I", len(members)))
    for path, stored, compressed, real in members:
        offset = len(data)
        data += stored
        tree += struct.pack("<I", len(path)) + path
        tree += struct.pack("<BIII", int(compressed), real, len(stored), offset)
    data += tree
    data += struct.pack("<II", len(tree), len(data) + 8)
    return bytes(data)


def fallout2_save(campaign=5):
    """A complete SPEC-018 section 7 save of `campaign` globals, written from the
    layout: the shape of section 8.9, one party member, and an empty inventory."""
    be = lambda *words: b"".join(struct.pack(">i", word) for word in words)
    header = bytearray(30051)
    header[:18] = b"FALLOUT SAVE FILE\0"
    header[24:28] = struct.pack(">HH", 1, 2)
    header[0x74E3:0x74F3] = b"VAULTENG F2SAVE1"
    header[0x74F3:0x74F7] = struct.pack(">I", campaign)
    header[0x74F7] = 1
    snapshot = lambda base: be(*range(base, base + campaign)) + b"\0"
    player = be(1, 20100, 0, 0, 0, 0, 0, 0, 0x01000000, 0, 0, 0x01000000, -1)
    player += be(0, 0, 0, -1, -1, 0, 1, 0)  # to the end of the common record
    player += be(0, 0, 0, 8, 0, 0, 0, -1, 30, 0, 0)  # the critter's own words
    discovery = bytes([2, 1, 0, 0, 0, 0, 0, 1, 1]) + bytes(33)
    world = be(175, 125, 0, 0, 0, -1, -1, 0) + struct.pack(">HH", 7, 6) + discovery
    world += be(1) + bytes([2, 0]) + be(175, 125) + bytes([3, 1, 0, 0])
    world += be(3) + bytes([0, 0, 3]) + be(2, 0, 1, 0, 0, 3, 1)
    member = be(0, 1, 0, 0, 0, 0, 3, 3, 4)
    return b"".join(
        (
            bytes(header),
            be(3),
            snapshot(0),
            be(1),
            b"VAULT13.SAV\0",
            be(797),
            snapshot(100),
            player,
            be(20100, 1, 8),  # viewport, sneak, stat flags
            be(*[5] * 35, *[1] * 35, *[2] * 18, 0, 0, 0),
            be(*[1] * 15, 7, 8, 9, 10),  # kills, with Fallout 2's four more
            be(0, 3, -1, -1),
            be(*[0] * 119),  # perk ranks
            be(0),  # not in combat
            be(4, 2, 1500, 3, -5),
            be(0),  # no timed events
            be(-1, -1, 0),  # traits, automap flags
            be(*[0] * 20),  # preferences
            be(1),
            b"\0",
            be(len(world)),
            world,
            bytes(17),  # movies
            be(*[0] * 54),  # skill uses
            be(2, 4000, 11),
            member,
            be(0),  # no retained records
            be(0, 0, 0, 0),
        )
    )


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
    write("int", "program.int", read(FIXTURES / "vm" / "program.int.hex"))
    for path in sorted((FIXTURES / "formats").glob("font.*.hex")):
        write("font", path.name.removesuffix(".hex"), read(path))
    for path in sorted((FIXTURES / "formats").glob("*.msg")):
        write("msg", path.name, path.read_bytes())
    for path in sorted((FIXTURES / "formats").glob("*.lst")):
        write("lst", path.name, path.read_bytes())
    for directory in ("world", "items"):
        for path in sorted((FIXTURES / directory).glob("*.pro.hex")):
            write("pro", path.name.removesuffix(".hex"), read(path))
    write("gcd", "player.gcd", read(FIXTURES / "state" / "player.gcd.hex"))
    for path in sorted((FIXTURES / "state").glob("*.gam")):
        write("gam", path.name, path.read_bytes())
    write("sve", "fixture-f.sve", (FIXTURES / "movie" / "fixture-f.sve").read_bytes())
    write("moviefx", "fixture-g.cfg", (FIXTURES / "movie" / "fixture-g.cfg").read_bytes())
    for path in sorted((FIXTURES / "loose" / "data").glob("ai*.txt")):
        write("ai", path.name, path.read_bytes())
    # SPEC-015: each zlib vector with its plain length, and small version 2
    # archives holding a stored and a compressed member.
    sizes = {"empty": 0, "text": 47, "words": 28011, "noise": 4096, "stored": 70000, "repeat": 40000}
    vectors = {}
    for line in read_text(FIXTURES / "zlib" / "vectors.hex").splitlines():
        if line.startswith("#") or not line.strip():
            continue
        if line.startswith("@"):
            name = line[1:].split()[0]
            vectors[name] = bytearray()
            continue
        vectors[name] += plain(line)
    for name, stream in vectors.items():
        write("zlib", name, struct.pack("<I", sizes[name]) + bytes(stream))
    for name in ("text", "repeat"):
        write("dat2", f"{name}.dat", dat2([(b"COLOR.PAL", b"pal", False, 3),
                                           (b"DATA\\" + name.upper().encode() + b".TXT",
                                            bytes(vectors[name]), True, sizes[name])]))
    write("fallout2_save", "synthetic.sav", bytes([0]) + fallout2_save())
    for path in sorted((FIXTURES / "spec024a").glob("vault-mod*.toml")):
        write("mod_manifest", path.name, path.read_bytes())
    tactical = ROOT / "mods" / "tactical-camera" / "vault-mod.toml"
    write("mod_manifest", "tactical-camera.toml", tactical.read_bytes())
    # The selector byte first: word set, enhanced range, mod words.
    program = read(FIXTURES / "vm" / "program.int.hex")
    for selector, name in ((0, "fallout1"), (1, "fallout2"), (2, "sfall"), (5, "enhanced")):
        write("stack_check", f"program-{name}.int", bytes([selector]) + program)
    registering = read(FIXTURES / "spec024a" / "gl_cheapap.int.hex")
    write("stack_check", "gl_cheapap.int", bytes([1 | 8]) + registering)
    for directory in sorted(CORPUS.iterdir()):
        print(f"{directory.relative_to(ROOT)}: {len(list(directory.iterdir()))} seeds")


if __name__ == "__main__":
    sys.exit(main())
