# SPEC-013 armed explosive fixture

Synthetic. Team A wrote `armed-explosive-door.tsv` from SPEC-011 section 3.24 and
SPEC-013 section 3.10. It names no game map or script. The `fallout2` and `fallout1`
columns give the expected result of one scene under each profile. Distances are hex
distances from the center hex, on one elevation. The two damage draws are stubbed.

`armed-explosive-door-rev10.tsv` (R1-rev10) gives the same scene's result under both
profiles, because the blast radius and fixed parameter 6 now hold under both. It
supersedes the `fallout1` column of `armed-explosive-door.tsv`, which stays until the
implementer's follow-up switches the tests.

`metarule49-door.tsv` is a door whose `damage_p_proc` asks `metarule` 49 about itself,
as bytes and as expected results. Team A stepped the fragment by a small interpreter
written from SPEC-004.
