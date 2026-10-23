# Korpus

A small, fast, self-hostable calorie and macronutrient tracker. Find a food,
check its nutrition, log what you ate and see your day add up. You can also
track your weight and set a calorie target if you want one.

- **Private.** Your diary, weight and profile stay in one SQLite file on your server. There is no analytics, tracking or AI.
- **Works offline from providers.** Products are looked up in [Open Food Facts](https://world.openfoodfacts.org) when it's enabled. Your own foods, your diary and everything else work when it is down or turned off.
- **Honest data.** Every food shows where its values come from. Missing values are flagged and never made up. Past diary days never change when you edit or delete a food.
- **English, French and Spanish.** The interface follows your browser's language or the one you pick in Settings. Food names are never machine-translated: product databases are searched in your language and return the names written in it.
- **Lightweight.** One Rust binary with server-rendered HTML and about 6 KB of optional JavaScript.

> Calorie estimates are approximate and are not medical advice.

## Quick start

### Docker Compose

```sh
cp .env.example .env        # optional: adjust settings
docker compose up -d --build
```

Open <http://localhost:9874>. The database is stored in the `korpus-data`
volume.

### From source

Requires Rust 1.88 or newer (developed with 1.96).

```sh
cargo build --release
./target/release/korpus      # listens on http://127.0.0.1:9874
```

With the default settings the database is created at `./data/korpus.db`.

### First run

1. Choose a language (or keep following the browser), metric or imperial units and your time zone. The time zone decides when a new diary day starts.
2. You can set a calorie goal now, or skip it.
3. You land on today's diary. Press **Add food**, search for a product or create it from its label, enter the amount and add it to a meal.

## Configuration

All settings are environment variables. See [`.env.example`](.env.example).

| Variable | Default | Purpose |
|---|---|---|
| `KORPUS_BIND` | `127.0.0.1:9874` | Address and port to listen on |
| `KORPUS_DATABASE_URL` | `sqlite://data/korpus.db` | SQLite database file |
| `KORPUS_DEFAULT_TIMEZONE` | `UTC` | Time zone suggested at first-run setup |
| `KORPUS_OFF_ENABLED` | `true` | Turn Open Food Facts lookups on or off |
| `KORPUS_OFF_BASE_URL` | `https://world.openfoodfacts.org` | Product (barcode) API |
| `KORPUS_OFF_SEARCH_URL` | `https://search.openfoodfacts.org` | Full-text search API |
| `KORPUS_CONTACT` | *(empty)* | Contact added to the User-Agent sent to Open Food Facts |
| `KORPUS_PROVIDER_TIMEOUT_SECS` | `8` | Timeout for provider requests (1–60) |
| `KORPUS_LOG` | `korpus=info,tower_http=warn` | Log filter |

Korpus will not start if a value is invalid, and it logs which variable is
wrong.

## Security and network exposure

Korpus is built for **one person per installation** and has **no login**.
Anyone who can reach the port can read and change your data. By default it
listens on `127.0.0.1` only.

To use it from other devices, put it behind a reverse proxy that handles TLS
**and authentication**, for example HTTP basic auth, an SSO proxy, or a VPN
such as WireGuard or Tailscale. Do not expose an unauthenticated instance to
the internet.

Example with Caddy (automatic TLS plus basic auth):

```caddy
korpus.example.org {
    basic_auth {
        # caddy hash-password
        me $2a$14$replace-with-your-hash
    }
    reverse_proxy 127.0.0.1:9874
}
```

Example with nginx:

```nginx
location / {
    auth_basic "Korpus";
    auth_basic_user_file /etc/nginx/korpus.htpasswd;
    proxy_pass http://127.0.0.1:9874;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-Host $host;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

Other built-in protections:

- **Cross-site request forgery.** Every change request is checked against the browser's `Sec-Fetch-Site` and `Origin` headers, and cross-site requests are rejected. Your proxy must pass the original `Host` (or `X-Forwarded-Host`).
- **Security headers.** A strict Content-Security-Policy with no inline scripts or styles, `X-Frame-Options: DENY`, `nosniff` and `Referrer-Policy: same-origin`.
- **Limits.** Request bodies, queries, page sizes and provider calls are all bounded. SQL is always parameterized and templates escape their output.
- **Logs.** Log lines record the method and path only. Search terms, diary contents and weights are never logged.

## External network use

Korpus makes outgoing requests only to the food databases you enable:

| Provider | When | What is sent |
|---|---|---|
| Open Food Facts | You pause typing for 300 ms on a search of 2 or more characters, submit a search or select a product | The search words or barcode, the interface language code (`en`, `fr` or `es`), plus a User-Agent naming Korpus |

Results refresh 300 ms after the last keystroke, for your own foods and the
food databases alike (queries of at least 2 characters). Requests that a newer
query has replaced are cancelled, and results are cached. Diary, weight
and profile data are never sent anywhere. The browser loads no third-party
scripts, fonts or images. Set `KORPUS_OFF_ENABLED=false` to turn the provider
off completely.

### Appearance

The interface follows the device's light or dark setting. **Settings →
Appearance** can force Light or Dark (true black, for OLED screens) on one
device; the choice is stored in a `korpus_theme` cookie that holds nothing
else and never leaves your server. Protein, carbohydrate and fat each keep
one colour everywhere (pink, orange and cyan), always next to their name or
letter so colour is never the only cue.

### Languages and food names

The interface language is a setting (**Settings → Language**). Left on
"Same as browser", it follows the browser's `Accept-Language` header and
falls back to English.

Food names follow the interface language without machine translation.
Searches ask Open Food Facts to match product names written in that language
(`langs=fr`), and every product comes with the official names its
contributors wrote in English, French and Spanish (`product_name_en`,
`product_name_fr`, `product_name_es`). Korpus stores all of them with a saved
product and shows the one for the current language everywhere: search
results, the food page and diary entries. When a product has no name in that
language, its main name is shown. Local search matches every stored name, so
a product saved in French is still found when searching in Spanish.

Foods you create keep exactly the name you typed. Renaming a saved product
replaces its official names, and your name is then used in every language.
Products saved before this feature get their names the next time they
appear in a database search.

### Search

Search shows one list: your saved foods first, then products from every
enabled food database that you have not saved yet. Results refresh 300 ms after you stop typing: your
foods first, then database results added to the same list. Selecting a
database product saves it to your foods.

Korpus enforces Open Food Facts' published request limits on its side (about
10 searches and 100 product reads per minute). When fast or slow typing uses
up the search budget, a search waits up to 3 s for a free slot; after that the
page says more results will appear and retries every 5 s until they do.
Results are cached in memory (1 h for searches, 24 h for products), and a
failed request is retried once at most.

### Data licensing

Open Food Facts data is © Open Food Facts contributors under the
[Open Database License (ODbL)](https://opendatacommons.org/licenses/odbl/1-0/);
product images are not used. Korpus shows attribution next to this data.
Products you select are saved in your local database. If you **redistribute**
a database or export that contains Open Food Facts products, the ODbL
share-alike and attribution terms apply to that data. See
<https://world.openfoodfacts.org/terms-of-use>.

## Backup and restore

All data lives in one SQLite database (plus its `-wal`/`-shm` files while
Korpus is running).

**Backup (recommended): Settings → Your data → Database backup.** This
downloads a consistent snapshot made with SQLite's `VACUUM INTO`, which is
safe while Korpus keeps writing. To automate it:

```sh
curl -fsS -o "korpus-$(date +%F).db" http://127.0.0.1:9874/settings/backup.sqlite
```

Without the web UI, use the `sqlite3` tool on the host (it is safe on a live
database too):

```sh
sqlite3 /path/to/korpus.db "VACUUM INTO '/backups/korpus-$(date +%F).db'"
```

Do **not** just copy `korpus.db` while Korpus is running: the copy can be
inconsistent.

**Restore:**

1. Stop Korpus (`docker compose stop`).
2. Replace `korpus.db` with the backup file, and delete any `korpus.db-wal` and `korpus.db-shm` next to it.
3. Start Korpus. Pending migrations run automatically.

With Docker, the database is at `/data/korpus.db` in the `korpus-data` volume:

```sh
docker compose stop
docker run --rm -v korpus_korpus-data:/data -v "$PWD":/backup debian:bookworm-slim \
  sh -c 'cp /backup/korpus-2026-10-22.db /data/korpus.db && rm -f /data/korpus.db-wal /data/korpus.db-shm && chown 10001:10001 /data/korpus.db'
docker compose start
```

The volume name depends on your compose project directory; check it with
`docker volume ls`.

**Exports:** Settings also offers a full JSON export and a CSV of your diary
for spreadsheets. A JSON export can be imported into a new, **empty**
installation. Import never merges into existing data, and an invalid file
changes nothing.

## Upgrades

1. Back up (see above).
2. `git pull && docker compose up -d --build` (or rebuild the binary and restart it).
3. Migrations in [`migrations/`](migrations) run automatically at startup. They only add to the schema, and startup never deletes data. If a migration fails, Korpus refuses to start and leaves the database unchanged, so restore the backup or fix the issue and restart.

Downgrades are not supported. To go back, restore the backup taken before the
upgrade.

## Health checks

- `GET /healthz`: liveness. Returns `ok` while the process runs.
- `GET /readyz`: readiness. Returns `ready` when the database answers, otherwise 503.
- `korpus healthcheck`: exits 0 when `/readyz` is ready. The Dockerfile uses this, so the image does not need curl.

## How nutrition is handled

- Values are stored **per 100 g or per 100 ml**, and the basis is always shown. Grams and millilitres are never converted into each other, because that would need a density Korpus doesn't have.
- An amount is calculated as `nutrient_per_100 × amount / 100`. Values are stored unrounded and rounded only for display (kcal to whole numbers, grams to one decimal).
- Energy given in kJ is converted with 1 kcal = 4.184 kJ, and the food notes that the value was converted.
- Values from a provider are validated. Negative, non-numeric or impossible values (for example more than 100 g of protein per 100 g) are dropped, flagged as missing and listed in the food's notes. Unknown values stay unknown and are never shown as 0. A day's total is marked incomplete when an entry is missing values.
- For Open Food Facts products, the per-100 ml basis is inferred from the package quantity (such as "1 l"), and the food's notes say so.
- Every diary entry keeps a **snapshot** of the food's name and nutrition. Editing or deleting a food never changes days you already logged.
- Foods are shared within the installation. There is one local profile.

### Calorie target

The optional estimate uses the **Mifflin–St Jeor** equation for resting
energy, times a standard activity factor (1.2–1.9). For weight loss it
subtracts 250 or 500 kcal per day, but never suggests less than your
estimated resting energy or 1200 kcal. Korpus gives no estimate for people
under 18. It suggests no weight-loss target if you indicate pregnancy,
breastfeeding or a relevant medical condition, and recommends professional
advice instead. Custom targets must be between 800 and 6000 kcal, and you get
a caution below 1200. The target is recalculated only when you save the goal
form, never when you log a new weight.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

The tests use real migrations on in-memory and temporary SQLite databases,
and a fake provider with saved Open Food Facts responses in `tests/fixtures`.
They never call live APIs.

Code layout (`src/`):

| Module | Responsibility |
|---|---|
| `config` | Environment configuration and validation |
| `db` | Connection setup (WAL, foreign keys) and migrations |
| `nutrition` | Bases, nutrient scaling, kJ/kcal, validation, totals, display rounding |
| `foods` | Food model, manual-entry validation, normalized local search, persistence |
| `providers` | Provider trait, Open Food Facts adapter, TTL cache, rate limiter |
| `diary` | Meals, entries with nutrition snapshots, daily totals |
| `weight` | Weight history (canonical kg) |
| `goals` | Mifflin–St Jeor estimate, safety rules, goal settings |
| `units` | Metric/imperial conversion, calendar dates per time zone, input parsing |
| `backup` | JSON/CSV export, JSON import, SQLite snapshots |
| `i18n` | Interface languages, `Accept-Language` negotiation, French and Spanish catalog |
| `web` | Routes, handlers, CSRF, language and header middleware |

Templates are in `templates/` (Askama, compiled into the binary). CSS and JS
are in `static/` and are embedded in the binary too.

### Translations

Interface text is written in English in the code and templates and passed
through `lang.t("…")` (or `lang.f("… {0}", …)`, `lang.f1`, `lang.f2` with
placeholders). The English text is the key into `CATALOG` in
`src/i18n.rs`, which holds the French and Spanish versions. A test fails if
any string used in `src/` or `templates/` is missing from the catalog, or
if a translation's placeholders don't match. Never pass food names, brands
or notes through `t()`.

### Adding a food provider

Implement `providers::FoodProvider` in a new module. Map the provider's data
to `ProviderFood`, keeping its own types private to that module. Use the
`Lang` passed to `search`, `get` and `lookup_barcode` to query and return
names in that language where the provider supports it. Then
register it in `Providers::from_config` behind its own enable flag, and add
fixture-based parser tests. Diary and food logic need no changes.

## Not included (by design)

There is no AI or photo-based food recognition, no AI-generated nutrition
estimates, no social features, no gamification and no subscriptions.

Planned after the MVP: barcode scanning with the camera (typing a barcode in
search already works), favourite and recent foods, recipes, copying a
previous day, and an installable PWA.
