# SPEC-011 item fixtures

Synthetic item prototypes for SPEC-011 §4. Every file is a complete,
big-endian `.PRO` image in the sparse hex form of
`tests/fixtures/formats/support.rs` (`offset: bytes`, every byte covered).
None of them reproduce original item statistics, prices, or names.

| File | PID | Meaning |
| --- | --- | --- |
| `drug-A.pro.hex` | `0x400` | Fixture A drug: IN/AG immediate +2/+1, stage 1 at 60 min (-1/-1), stage 2 at 120 min (-1 IN), 20% addiction, withdrawal perk 55, onset 1440 min |
| `drug-B.pro.hex` | `0x401` | Fixture B drug: random-range form (S0 = -2), current HP `roll(10, 20)`, no delayed stages, 0% addiction |
| `ammo-X.pro.hex` | `0x500` | Fixture E ammunition: weight 1, cost 50, caliber 7, pack size 20 |
| `weapon-W.pro.hex` | `0x501` | Fixture E weapon: weight 5, cost 300, caliber 7, capacity 12, default ammunition `0x500`, minimum strength 6 |
| `container.pro.hex` | `0x504` | Fixture E container: maximum size 10 |
| `armor-A.pro.hex` | `0x502` | Fixture I armor A: AC 20, DR normal 30 / EMP 500, DT normal 4, no perk |
| `armor-B.pro.hex` | `0x503` | Fixture I armor B: AC 25, DR normal 40 / EMP 500, DT normal 12, perk 62 |

`support.rs` loads them and supplies the synthetic message catalogs
(`item.msg`, `perk.msg`, `stat.msg`, `proto.msg`, `inventry.msg`,
`misc.msg`) whose texts are placeholders with the same `%d`/`%s`
conversions as the message numbers SPEC-011 names.
