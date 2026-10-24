# Ciqual 2025 extract

`ciqual_2025_11_03.tsv` is an extract of the French food composition table
published by ANSES. Korpus compiles it into the binary and installs it on first
start, so generic foods (fruit, vegetables, eggs, milk…) can be found without
downloading anything.

## Source

Anses. 2025. Table de composition nutritionnelle des aliments Ciqual 2025.
Recherche Data Gouv. <https://doi.org/10.57745/RDMHWY>

- Release: Ciqual 2025, files dated 2025-11-03, published 2025-11-19.
- Producer: ANSES, Observatoire des Aliments.
- Files used, with their SHA-256 checksums:

  | File | SHA-256 |
  |---|---|
  | `alim_2025_11_03.xml` | `e0b1de25b3039028205e9d54a96892e403e1b313c2efeb41180fabe132627478` |
  | `compo_2025_11_03.xml` | `8c46a9032ece4eab4ffccc9dfcb0c490ec2f416aa664cc1ee013b241c6bdd4af` |

## Licence

The Ciqual table is published under the
[Licence Ouverte / Open Licence version 2.0 (Etalab)](https://www.etalab.gouv.fr/licence-ouverte-open-licence/)
(SPDX `etalab-2.0`). The licence allows reuse, including redistribution and
modification, as long as the source and the date it was last updated are
credited. This extract and the foods Korpus creates from it are under the same
licence. They are not covered by Korpus's own licence (AGPL-3.0), which applies
to the code only.

ANSES does not endorse Korpus. Korpus shows the attribution "ANSES-CIQUAL French
food composition table. Licence Ouverte / Open Licence (Etalab)." in Settings,
next to search results from the table, and on every food saved from it.

## What was changed

This file is a selection of the publisher's data, not a recalculation:

- Kept: each food's code (`alim_code`), its French and English names
  (`alim_nom_fr`, `alim_nom_eng`), and its values for the 10 constituents
  Korpus uses: energy in kcal (328) and kJ (327), protein (25000),
  carbohydrate (31000), fat (40000), fibre (34100), sugars (32000), saturated
  fatty acids (40302), salt (10004) and sodium (10110).
- Left out: every other constituent, the food groups, scientific names,
  min/max values, confidence codes and sources.
- Values are copied exactly as published, including their notation
  (`2,86`, `< 0,5`, `traces`, `-`). XML escapes are decoded (`&lt;` → `<`), and
  runs of whitespace are reduced to one space. This only affects one name,
  which has a line break in the publisher's file.
- Korpus then reads the values the same way as an upload of the full table.
  Values below a detection limit and traces are recorded as 0 and noted, and
  `-` stays missing. Salt is derived from sodium, and kcal from kJ, only when
  the table gives no value. Any such conversion is noted on the food.

## Regenerating

Download `alim_*.xml` and `compo_*.xml` from the DOI page above, then:

```sh
cargo run -- dataset extract ciqual alim_2025_11_03.xml compo_2025_11_03.xml \
  > bundled/ciqual/ciqual_2025_11_03.tsv
```

For a new release, add the new file, update `BUNDLED` in
`src/datasets/bundled.rs` (version, label and file name), and update this
README. On their next start, installations that use the bundled table switch to
the new release. Installations where the administrator imported the table, or
where the user removed it, are left alone.
