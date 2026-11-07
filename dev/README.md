# mediabbs development environment

A complete local stack for developing and testing mediabbs against a real
MediaWiki with OAuth 2.0. **Not for production** (plain HTTP, fixed passwords).

| Service | URL | What |
|---|---|---|
| client  | http://127.0.0.1:18090 | mediabbs-client (nginx) |
| backend | http://127.0.0.1:18081 | mediabbs API |
| wiki    | http://127.0.0.1:18100 | MediaWiki 1.43 + Extension:OAuth (SQLite, Korean UI) |
| db      | (internal) | MariaDB for mediabbs |

All ports are bound to 127.0.0.1 only.

## Start

The client is built from `../../mediabbs-client` (a sibling checkout); set
`MEDIABBS_CLIENT_DIR` to use another path.

```sh
cd dev
docker compose up -d --build
docker compose logs -f wiki backend   # first start installs the wiki (~30 s)
```

Open http://127.0.0.1:18090 and log in with one of the test accounts:

| User | Password | Notes |
|---|---|---|
| Admin | `mediabbs-dev-admin` | sysop -> board moderator; has an email address |
| Moderator | `mediabbs-dev-user` | member of `boardmanager` -> moderator via `ADMIN_GROUPS` |
| Alice | `mediabbs-dev-user` | regular user |
| Blocked | `mediabbs-dev-user` | blocked on the wiki, cannot post |

Boards `free` (자유게시판) and `archive` (보관함, disabled/read-only) are
created automatically.

## What the wiki container sets up

`mediawiki/setup.sh` runs on first start (state lives in the `wiki-data` volume):

1. Installs MediaWiki with SQLite (`--lang ko`; set `WIKI_LANG=en` for English).
2. Generates the OAuth 2.0 RSA key pair (`$wgOAuth2PrivateKey`/`$wgOAuth2PublicKey`).
3. Loads Extension:OAuth and grants sysops the consumer management rights.
4. Creates the test users above and blocks `Blocked`.
5. Registers and approves two OAuth 2.0 consumers with callback
   `http://127.0.0.1:18090/auth`, using `mediawiki/createOAuth2Consumer.php`
   (the bundled `createOAuthConsumer.php` only creates OAuth 1.0a consumers):
   - `mediabbs-dev`: identity + real name/email (`authonlyprivate`) - used by default
   - `mediabbs-dev-authonly`: identity only (no email)
6. Writes the client id/secret to `/shared/oauth.env` and the public key to
   `/shared/pubkey.pem`, which the backend container reads at startup.

The same steps apply to a real wiki; see the installation guide.

Container environment (first-start values): `WIKI_SERVER`, `WIKI_NAME`, `WIKI_LANG`,
`BBS_CALLBACK`, `ADMIN_PASSWORD`, `USER_PASSWORD`, `OAUTH2_ACCESS_TOKEN_TTL`.
Extra PHP settings can be mounted as `/var/www/conf.d/*.php`; they are loaded after
`LocalSettings.php` on every request (restart the container to bypass opcache).

## End-to-end test

```sh
cd dev/e2e
npm install
npx playwright install chromium-headless-shell
node oauth-e2e.js
node ui-e2e.js
node admin-e2e.js
node search-e2e.js
node live-e2e.js
```

`oauth-e2e.js` logs in through the wiki as each test user (including the
consent screen) and checks moderation, blocks, uploads, token refresh, logout,
anonymous posting and disabled boards. `ui-e2e.js` drives the client: editor
modals, quoting, the moderation menu, infinite scroll, error messages, the
language switch and both skins (Liberty, Simple). `admin-e2e.js` checks the
admin page: board settings (icon, separator, order, menu visibility, write
permissions, deletion) and per-skin menus, and how both skins show them. It
leaves a hidden test board at the end of the board list. `search-e2e.js` checks
search: titles, comments, authors, filters, the search page and the board
search boxes of both skins. `live-e2e.js` checks live updates (new and hidden
comments, topic status, background tabs) and the polling fallback.

### More manual checks

- **Automatic token refresh**: start the wiki with one-minute access tokens,
  log in, wait a minute and write a post; the client gets `401 TokenExpired`,
  calls `/auth/refresh` and retries.
  ```sh
  OAUTH2_ACCESS_TOKEN_TTL=PT1M docker compose up -d --no-build wiki
  ```
- **IP blocks for anonymous users**: the backend sees requests from Docker's
  gateway address (printed by the e2e test, e.g. `10.213.0.1`).
  ```sh
  echo 10.213.0.1 | docker compose exec -T wiki php maintenance/blockUsers.php --performer Admin --reason test
  echo 10.213.0.1 | docker compose exec -T wiki php maintenance/blockUsers.php --performer Admin --unblock --reason done
  ```
- **Identity-only consumer**: copy the commented credentials from
  `docker compose exec wiki cat /shared/oauth.env` into the backend
  environment; `GET /me` then returns an empty `email`.

## Reset

```sh
docker compose down -v   # removes the wiki, database and uploads volumes
```
