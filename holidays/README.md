# Holiday packs

`<COUNTRY>[-<REGION>]/<YEAR>.csv` with header `date,name,kind` (`kind` = `mandatory` | `optional`).
Packs are embedded in the binary and offered in the setup wizard and `/admin/holidays/import`.
CI (`scripts/validate_holiday_packs.py`) checks the format, that every date is in the file's
year, and that no `(date, name)` pair repeats. Every pack must name its source below.

| Pack | Source |
|---|---|
| `IN/2026` | Gazetted holidays for Central Government offices, DoP&T O.M. F.No.12/2/2023-JCA dated 03.07.2025 |
| `IN/2027` | Gazetted holidays for Central Government offices at Delhi/New Delhi, DoP&T O.M. F.No.12/2/2023-JCA dated 16.07.2026 (Annexure I) |

Companies usually observe only some of these; admins mark the rest optional or delete them
after importing.
