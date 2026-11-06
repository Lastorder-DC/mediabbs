# librewiki-specific files

mediabbs is a general-purpose fork of [LibreBBS](https://github.com/librewiki/librebbs).
Everything that only makes sense for [librewiki](https://librewiki.net)
(bbs.librewiki.net) lives here instead of in the default installation.

| File | Purpose |
|---|---|
| `seed_boards.sql` | librewiki's five boards (위키방, 자유게시판, 익명게시판, 조합게시판, 이슈트래커). Formerly inserted by migrations. |
| `migrate_from_xe.sql` | One-off import from librewiki's old XpressEngine board. Reference only. |
| `env.librewiki` | Settings that reproduce the original librebbs behaviour against librewiki.net. |

## Reproducing the librewiki setup

```sh
mediabbs migrate
mysql <database> < contrib/librewiki/seed_boards.sql
cp contrib/librewiki/env.librewiki .env   # then fill in the secrets
mediabbs
```

## Upgrading an existing librebbs database

No database changes are needed: migrations that used to insert boards are
now no-ops, and diesel does not re-run migrations that were already applied.
Only the environment variables changed; see `env.librewiki`.
