# Fixture PRD

## R96 — Old style (M32)
- R96-a: an old AC without the 1.0 format is ignored.
- R96-b [M36] (unit): Given an old section. When traced. Then it is ignored.

## R97 — Source links (M36)
Some prose that mentions R97-a and - R97 without being an AC.
- R97-a [M36] (unit): Given a link. When resolved. Then the base is `develop`.
- R97-b [M36] (e2e): Given a link. When shown. Then the table has 3 rows.
- R97-c [M36] (unit+e2e): Given a comment. When traced. Then it does not count.
- R97-d [M36] (live): Given a multi-ID test name. When traced. Then both IDs count.
- R97-e [M36] (e2e): Given a single-quoted check. When traced. Then it counts.

## R98 — Checkouts (M37)
- R98-b [M37] (unit): Given letters out of order. When sorted. Then a comes before b.
- R98-a [M37]: Given no kind. When traced. Then it is still an AC.

## R99 — Old thing — Removed (1.0.0)
Removed in 1.0.0 (ADR-0033, R101).
- R99-a this line would be a format error if the section were traced.

## R100 — Old visual — Superseded (1.0.0) by R102
- R100-a broken too, but skipped.

# Non-goals
- R97-z [M36] (unit): Given text outside any R section. When traced. Then ignored.
