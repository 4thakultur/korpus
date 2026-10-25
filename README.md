# Korpus

A small, fast, self-hostable calorie and macronutrient tracker. Find a food,
check its nutrition, log what you ate and see your day add up. You can also
track your weight, set a calorie target and plan your gym programs if you
want to.

- **Private.** Your diary, weight and profile stay in one SQLite file on your server, behind a login with optional two-factor authentication. There is no analytics, tracking or AI.
- **Works offline from providers.** Products are looked up in [Open Food Facts](https://world.openfoodfacts.org) when it's enabled. Your own foods, your diary and everything else work when it is down or turned off.
- **National food tables, searched locally.** The French table Ciqual 2025 is included and installed on first start, so fruit, vegetables, eggs or milk can be found without picking a brand. You can also import the official table for your country (France, UK, USA, Canada or Australia). Korpus picks the one that fits the country set in your profile. Generic foods such as "apple, raw" then come with laboratory values, and searching them never leaves your server.
- **Honest data.** Every food shows where its values come from. Missing values are flagged and never made up. Past diary days never change when you edit or delete a food.
- **English, French and Spanish.** The interface follows your browser's language or the one you pick in Settings. Food names are never machine-translated: product databases are searched in your language and return the names written in it.
- **Lightweight.** One Rust binary with server-rendered HTML and about 6 KB of optional JavaScript.

> Calorie estimates are approximate and are not medical advice.

## Quick start

### Docker Compose

```sh
cp .env.example .env        # optional: adjust settings
mkdir -p data               # before the first start, so it belongs to you
docker compose up -d --build
```

Open <http://localhost:9874>. The database is stored in `./data` (set
`DATA_DIR` in `.env` to put it elsewhere). The container runs as `PUID:PGID`
(default `1000:1000`), which must be able to write that directory; Docker
would create a missing one owned by root.

The web UI is published on `127.0.0.1` only. When your reverse proxy runs in
another container or on another machine, it cannot reach that address: set
`KORPUS_HTTP_BIND=0.0.0.0` in `.env` (or another interface address), and read
[Security and network exposure](#security-and-network-exposure) first.
`KORPUS_HTTP_PORT` changes the published port.

### From source

Requires Rust 1.88 or newer (developed with 1.96).

```sh
cargo build --release
./target/release/korpus      # listens on http://127.0.0.1:9874
```

With the default settings the database is created at `./data/korpus.db`.

### First run

1. **Create your login.** Korpus asks for a username, a password (at least 10 characters) and a *setup code*. The code is printed in the server log when Korpus starts (`docker compose logs korpus`, or the terminal running it), so only the person running the server can create the account, even if someone else reaches a new instance first. It is only needed once.
2. Choose a language (or keep following the browser; the page switches to it at once), your country, metric or imperial units and your time zone. The country picks the food composition table search uses. The time zone decides when a new diary day starts.
3. A few questions about your usual activity, one per screen: roughly how many steps you take a day; whether you train at the gym, and if so on which days of the week, for how long and whether to start from a program template (Push / Pull / Legs, Upper / Lower, Full body or 5×5; "Not now" leaves the Gym empty); and whether you play a sport, and if so which one, on which days and for how long. The gym is not among the sports, since it is planned with programs under Activity › Gym. Tap an answer to move on; every question can be skipped. The answers suggest the activity level in Goals (see [Calorie target](#calorie-target)) and the sport offered first in Activity. The gym and the sport are also added to Activity as sessions repeating every week on the chosen days, starting today, so the energy they use is added to each of those days' totals; change, end or skip a day there like any repeating activity. A program template is placed on the chosen gym days when it has as many sessions a week; otherwise it becomes a rotation, done in order on whichever days you train.
4. You can set a calorie goal now, or skip it.
5. You land on today's diary. Press **Add food**, search for a product or create it from its label, enter the amount and add it to a meal.

## Configuration

All settings are environment variables. See [`.env.example`](.env.example).

| Variable | Default | Purpose |
|---|---|---|
| `KORPUS_BIND` | `127.0.0.1:9874` | Address and port to listen on |
| `KORPUS_ALLOWED_HOSTS` | *(empty)* | Host names Korpus answers to besides IP addresses and `localhost`, comma-separated (see below) |
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

Korpus is built for **one person per installation**. Every page needs a
login, except the login pages themselves, the stylesheet and script, and the
health checks. By default it listens on `127.0.0.1` only.

### Login and two-factor authentication

- **Password.** Stored as an Argon2id hash, never in plain text. Change it in **Settings → Account → Change password**; other devices are logged out.
- **Two-factor authentication (optional).** **Settings → Account → Two-factor authentication** asks for your password, then shows a QR code for an authenticator app (Aegis, 2FAS, Google Authenticator, 1Password, …). Each start makes a new key, and the key is never shown again afterwards. Once it's on, logging in asks for the app's 6-digit code after the password. Each code works only once. Turning it on logs out other devices; turning it off needs the password.
- **Sessions.** A login lasts 30 days and is extended while you use Korpus. The browser keeps a random token in an `HttpOnly`, `SameSite=Lax` cookie, which also gets `Secure` when your reverse proxy sends `X-Forwarded-Proto: https`. The database stores only a hash of each token, and backups leave sessions out, so a backup cannot be used to log in and restoring one does not bring back logged-out devices. **Settings → Account → Log out** ends the session.
- **Guessing.** After 10 failed passwords, codes or setup codes from one address within 15 minutes, that address has to wait; requests sent at the same time count too. IPv6 addresses are counted per /64. There is also a limit of 100 failures across all addresses. Password checks run at most two at a time, so a flood of login requests cannot exhaust the server's memory. Failed logins are logged with the client address (`failed login`), for tools such as fail2ban.
- **Behind a reverse proxy**, every request comes from the proxy's address, so the limit would apply to everyone together: anyone could lock you out of logging in with 10 wrong passwords. List your proxy in `KORPUS_TRUSTED_PROXIES` (for example `127.0.0.1`, or `172.16.0.0/12` when the proxy reaches the Docker container through its published port) so Korpus uses the client address the proxy adds to `X-Forwarded-For`. List only proxies you run: a listed address can claim to be any client. Devices already logged in are never affected by the limit.

**Lost access?** These commands need access to the server, not to the web page.
With Docker, run them as `docker compose exec korpus korpus account …`:

```sh
korpus account disable-2fa   # lost the phone: log in with the password only
korpus account reset         # forgot the password: removes the login, keeps all diary data
```

After `reset`, the next visit asks you to create a new login, with a new setup
code from the server log.

Korpus only answers requests addressed to an IP address, `localhost`, or a
host name listed in `KORPUS_ALLOWED_HOSTS`. When you open it by name (for
example `https://korpus.example.org` or `http://korpus.lan:9874`), add that
name, without scheme or port:

```sh
KORPUS_ALLOWED_HOSTS=korpus.example.org,korpus.lan
```

Other names get `421 Misdirected Request`. This stops *DNS rebinding*, where a
website you visit points its own domain at your server's address so that your
browser lets its scripts read and change your diary. If a reverse proxy
rewrites `Host` (nginx does unless told otherwise) the name it sends must be
allowed too; passing the original `Host` as in the examples below avoids that.

To use it from other devices, put it behind a reverse proxy that handles
TLS, or reach it through a VPN such as WireGuard or Tailscale. Without TLS,
your password and session cookie cross the network unencrypted. For an
instance reachable from the internet, turn on two-factor authentication. You
can add a second layer such as the proxy's basic auth, an SSO proxy or a VPN.

Example with Caddy (automatic TLS, plus optional basic auth as a second layer):

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

- **Login.** See [Login and two-factor authentication](#login-and-two-factor-authentication) above.
- **Cross-site request forgery.** Every change request is checked against the browser's `Sec-Fetch-Site` and `Origin` headers, and cross-site requests are rejected. Your proxy must pass the original `Host` (or `X-Forwarded-Host`).
- **DNS rebinding.** Requests for unknown host names are refused (see `KORPUS_ALLOWED_HOSTS` above).
- **Security headers.** A strict Content-Security-Policy with no inline scripts or styles, `X-Frame-Options: DENY`, `nosniff`, `Referrer-Policy: same-origin` and same-origin `Cross-Origin-Opener-Policy` / `Cross-Origin-Resource-Policy`.
- **Limits.** Request bodies, queries, page sizes and provider calls are all bounded. Provider responses are capped at 8 MB and redirects are only followed within the same host. Uploaded spreadsheets and archives are checked for decompression bombs, and only one import or backup runs at a time. SQL is always parameterized and templates escape their output.
- **Backups.** The temporary copy made for **Download backup** is written to a directory only the Korpus user can read and deleted right after.
- **Container.** `compose.yaml` runs Korpus as an unprivileged user (`PUID:PGID`) with a read-only root filesystem, no Linux capabilities and `no-new-privileges`; only `/data` and a `/tmp` tmpfs are writable.
- **Logs.** Log lines record the method and path only. Search terms, diary contents and weights are never logged.

## External network use

Korpus makes outgoing requests only to the food databases you enable:

| Provider | When | What is sent |
|---|---|---|
| Open Food Facts | You pause typing for 300 ms on a search of 2 or more characters, submit a search or select a product | The search words or barcode, the interface language code (`en`, `fr` or `es`), plus a User-Agent naming Korpus |

Imported food composition tables are searched in the local database and are
never contacted: they make no network requests at all.

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

Search shows one list: your saved foods first, then foods from the food
composition table chosen for your country, then products from every enabled
food database that you have not saved yet. Searching for a barcode skips the
tables, which hold no packaged products. Results refresh 300 ms after you stop typing: your
foods first, then database results added to the same list. Selecting a
database product saves it to your foods.

Each part of the list is ranked the same way:
- how many of your words appear in the name or brand, counting plurals
  ("oeufs" finds "Oeuf, cru"), accents, ligatures ("oeufs" finds "Œufs") and
  words written apart or together ("pompotes", "Pom'Potes");
- how closely the name matches what you typed. Words such as "cru",
  "cuit" or "pulpe" don't count against a name, and a table's default entry
  ("Fromage (aliment moyen)") comes first for the food it names;
- names that start with your first word, or contain your words in the order
  you typed them, rank higher;
- popularity: how often you have logged a saved food, or how often an Open
  Food Facts product has been scanned. The well-documented entry comes before
  its one-scan duplicates;
- whether an Open Food Facts product has an energy value.

Saved foods match any part of a word ("apple" finds "Pineapple", ranked
lower). Food composition tables match the start of words, so "eau" doesn't
list every "gâteau".

Open Food Facts results are re-ordered by Korpus. Each search fetches 100
candidates (enough for five result pages) and ranks them. Copies of the same
product under different barcodes (same name, brand, energy and macronutrients)
are shown once.

If no candidate contains every word but your search names a brand or a
store's own label (for example "picard pavé saumon"), Korpus makes one more
search restricted to that brand. That search uses one more request from the
per-minute budget and is skipped when the budget is used up.

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

The sport icons (`static/sports.svg`) come from
[Tabler Icons](https://tabler.io/icons), © Paweł Kuna, under the MIT License.

## Food composition tables

Open Food Facts covers packaged products well but generic foods poorly. For
those, Korpus uses the official national food composition tables.

**Included: Ciqual 2025.** Korpus ships an extract of the French ANSES-CIQUAL
2025 table: 3,484 generic foods with French and English names. It is installed
the first time Korpus starts, and is used whenever no table that better fits
your country is imported (see the order below). It is installed only once. If
you remove it in Settings, it stays removed. If you imported CIQUAL yourself,
your import is never replaced. **Settings → Food composition tables → Use Ciqual
2025, included with Korpus** (or `korpus dataset install-bundled ciqual`)
installs it again. The extract, its source, its licence and what it leaves out
are described in [`bundled/ciqual/README.md`](bundled/ciqual/README.md).

The other tables, and newer CIQUAL releases, are imported from the
publisher's files. These are measured or officially compiled values with stable codes. You
download each file once from its publisher and import it. Korpus then
searches it in its own database, so no search ever leaves your server.

| Table | Country | Download | Licence |
|---|---|---|---|
| ANSES-CIQUAL (2025 included; 2020 or later) | France | [`alim_*.xml` and `compo_*.xml`](https://doi.org/10.57745/RDMHWY), or the 2020 XML zip | Licence Ouverte / Etalab 2.0 |
| CoFID, McCance and Widdowson's (2021) | United Kingdom | [Excel file](https://www.gov.uk/government/publications/composition-of-foods-integrated-dataset-cofid) | Open Government Licence v3.0 |
| USDA FoodData Central | United States | [Foundation Foods and SR Legacy CSV zips](https://fdc.nal.usda.gov/download-datasets) | Public domain (CC0 1.0) |
| Canadian Nutrient File (2015) | Canada | [CSV zip](https://www.canada.ca/en/health-canada/services/food-nutrition/healthy-eating/nutrient-data/canadian-nutrient-file-2015-download-files.html) | Open Government Licence – Canada |
| Australian Food Composition Database (Release 3) | Australia | [Excel file "Nutrient profiles"](https://www.foodstandards.gov.au/science-data/food-nutrient-databases/afcd/australian-food-composition-database-download-excel-files) | FSANZ data user licence; FSANZ does not endorse Korpus |

**Importing.** In **Settings → Food composition tables**, upload the file
exactly as downloaded (for USDA, upload the Foundation and SR Legacy zips
together). Or use the command line, for example for scripted installs:

```sh
korpus dataset import ciqual alim_2025_11_03.xml compo_2025_11_03.xml
korpus dataset import usda FoodData_Central_foundation_food_csv_*.zip FoodData_Central_sr_legacy_food_csv_*.zip
korpus dataset list
korpus dataset remove cnf

# With Docker Compose, copy the file into the container first:
docker compose cp XML_2020_07_07.zip korpus:/tmp/
docker compose exec korpus korpus dataset import ciqual /tmp/XML_2020_07_07.zip
docker compose exec korpus korpus dataset install-bundled ciqual
```

Importing a table again replaces it in one transaction; if the file is wrong,
the previous version stays. Removing a table keeps every food you already
saved from it, along with your diary.

**Which table is used.** Search uses one table: the first imported one in the
order that fits your profile's country.

| Your country | Order of preference |
|---|---|
| France, Belgium, Luxembourg, Monaco | CIQUAL, CoFID, USDA, CNF, AFCD |
| United Kingdom, Ireland | CoFID, CIQUAL, USDA, AFCD, CNF |
| Other European countries | CIQUAL, CoFID, USDA, AFCD, CNF |
| United States, Puerto Rico | USDA, CNF, CoFID, AFCD, CIQUAL |
| Canada | CNF, USDA, CIQUAL, CoFID, AFCD |
| Australia, New Zealand | AFCD, CoFID, USDA, CNF, CIQUAL |
| Other countries, or none set | USDA, CNF, CoFID, CIQUAL, AFCD |

European tables come first for European countries because they follow the
same labelling rules as the packages you buy there: EU energy factors, and
carbohydrate without fibre. Food names are shown in your interface language
when the table publishes it: CIQUAL and CNF have French and English names,
the others English only. Names are never machine-translated.

**What is converted, and how it is shown.** Values are stored as published,
per 100 g. CoFID's alcoholic drinks and AFCD's liquids are stored per 100 ml,
as those tables publish them. Each saved food's notes list every
conversion:

- Energy published only in kJ (AFCD, and the rare food elsewhere) is converted with 1 kcal = 4.184 kJ.
- Salt published only as sodium is calculated as sodium × 2.5.
- USDA and CNF carbohydrate is the North American "total by difference", which **includes fibre**, unlike European labels. It is kept as published and noted.
- Values published as "traces" or below a detection limit ("< 0.5", "Tr") are recorded as 0 and listed. Values marked not determined ("-", "N") stay missing. CIQUAL 2020, for example, publishes no energy value for about 900 foods (CIQUAL 2025 for about 150), and Korpus shows them as missing rather than computing one.
- Invalid values (negative, or impossible per 100 g) are dropped and listed, as for any provider. Carbohydrate and sugars may reach 106 g per 100 g, because UK tables express sugars as monosaccharide equivalents (100 g of sucrose is 105.3 g).

**Not supported yet.** Germany's BLS 4.0, the Netherlands' NEVO and
Finland's Fineli are freely usable, but their download files could not be
checked while this importer was written (a licence form, a request form and
a bot challenge respectively), so they have no importer. Spain's BEDCA has
no open bulk download. USDA branded foods (a multi-gigabyte file) are not
imported; Open Food Facts covers packaged products.

**Licences.** Each table's attribution is shown in Settings, next to search
results that come from it and on every food saved from it. The data is
yours to use under those licences. If you redistribute a database that
contains imported tables, include their attribution. The bundled Ciqual
extract is under the Licence Ouverte 2.0, not under Korpus's code licence.

## Backup and restore

All data lives in one SQLite database (plus its `-wal`/`-shm` files while
Korpus is running).

**Backup (recommended): Settings → Your data → Database backup.** This
downloads a consistent snapshot made with SQLite's `VACUUM INTO`, which is
safe while Korpus keeps writing. To automate it, use the command line (the
web download needs a login). It is also safe while Korpus runs:

```sh
korpus backup "/backups/korpus-$(date +%F).db"
# with Docker, into the data volume:
docker compose exec korpus korpus backup "/data/korpus-$(date +%F).db"
```

A backup contains your login too: the password hash and, if two-factor
authentication is on, the authenticator secret. Keep backups as private as
the server; `korpus backup` creates the file readable only by its owner and
never overwrites an existing one. Sessions are left out, so after restoring
you log in again.

Without the web UI, use the `sqlite3` tool on the host (it is safe on a live
database too):

```sh
sqlite3 /path/to/korpus.db "VACUUM INTO '/backups/korpus-$(date +%F).db'"
```

Such a copy still holds the sessions of that moment. After restoring one,
run `sqlite3 /path/to/korpus.db "DELETE FROM sessions"` (with Korpus
stopped) so devices logged out since then cannot get back in.

Do **not** just copy `korpus.db` while Korpus is running: the copy can be
inconsistent.

**Restore:**

1. Stop Korpus (`docker compose stop`).
2. Replace `korpus.db` with the backup file, and delete any `korpus.db-wal` and `korpus.db-shm` next to it.
3. Start Korpus. Pending migrations run automatically.

With Docker, the database is `korpus.db` in the data directory (`./data`
unless `DATA_DIR` says otherwise):

```sh
docker compose stop
cp korpus-2026-10-22.db data/korpus.db && rm -f data/korpus.db-wal data/korpus.db-shm
docker compose start
```

Copy as the user set in `PUID`/`PGID`, so Korpus can still write the file.

The database backup includes imported food composition tables. The JSON
export does not: import the tables again after restoring from JSON. Foods you
saved from them are part of the export.

**Exports:** Settings also offers a full JSON export and a CSV of your diary
for spreadsheets. A JSON export can be imported into a new, **empty**
installation. Import never merges into existing data, and an invalid file
changes nothing.

### Resetting the profile

**Settings → Preferences → Reset profile** erases everything you recorded:
foods, diary, weights, steps, activity, gym programs and workouts, goals and
settings. You stay logged in and go through first-run setup again. Your
username, password and two-factor authentication are kept, and so are the
food composition tables. It cannot be undone, so download a database backup
first if you may want the data back.

## Upgrades

1. Back up (see above).
2. `git pull && docker compose up -d --build` (or rebuild the binary and restart it).
3. Migrations in [`migrations/`](migrations) run automatically at startup. They only add to the schema, and startup never deletes data. When a new version changes how search text is normalized, the search index of saved and table foods is rebuilt once at startup (a few seconds for a full table). If a migration fails, Korpus refuses to start and leaves the database unchanged, so restore the backup or fix the issue and restart.

Downgrades are not supported. To go back, restore the backup taken before the
upgrade.

**Data directory (Docker):** `compose.yaml` used to keep the database in a
named volume (`korpus_korpus-data`); it now uses `./data`. To keep an existing
database, copy it over before starting the new version:

```sh
docker compose down
mkdir -p data
docker run --rm -v korpus_korpus-data:/old -v "$PWD/data":/new debian:bookworm-slim \
  sh -c 'cp -a /old/. /new/ && chown -R 1000:1000 /new'
docker compose up -d --build
```

Use your `PUID:PGID` instead of `1000:1000` if you changed them. Remove the old
volume (`docker volume rm korpus_korpus-data`) once Korpus works.

**Host name check:** Korpus now refuses requests for host names it does not
know. If you open it by a name rather than an IP address or `localhost`, set
`KORPUS_ALLOWED_HOSTS` to that name when you upgrade, or the browser will get
`421 Misdirected Request` (see [Security and network exposure](#security-and-network-exposure)).

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
- Foods from food composition tables keep the table's own basis and values; see [Food composition tables](#food-composition-tables) for the few conversions and how they are noted.
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

The activity level is suggested from the usual activity answered at first
run, unless one is already set. The answers become energy above rest in
MET-hours a week (about kcal per kg of body weight):

- steps beyond 5,000 a day, which *Sedentary* already includes, taking the
  middle of the chosen range (over 15,000 counts as 17,500), walked at 100
  steps a minute and 3.5 METs;
- the sport's MET value (as for recorded sport, see [Activity](#activity))
  minus 1, times sessions a week and session length;
- the gym the same way, as Traditional Strength Training (3.5 METs).

Every 20 MET-hours a week is one level above *Sedentary*, rounded to the
nearest. One level adds 17.5% of resting energy, roughly 27 MET-hours a week
for an average adult, so the suggestion leans slightly high of the arithmetic
while staying below the levels' descriptions, which tend to overestimate. For
example, 7,500 steps a day and running three times 45 minutes a week (about
7 + 18 MET-hours) suggest *Lightly active*. The suggestion is only a starting
point: change it in Goals at any time.

### Macronutrient goals

Protein, carbohydrate and fat goals can be entered by hand or estimated from
the daily calorie target, the goal and the weight in Goals:

- **Protein:** 1.2 g per kg of body weight to maintain, 1.4 g to lose
  slowly, 1.6 g to lose or to gain, plus 0.2 g for the *Very active* and
  *Extra active* activity levels. It never takes more than 35% of the
  energy target. All rates are above the 0.8 g/kg adult reference intake:
  more protein helps keep lean mass during a deficit.
- **Fat:** 30% of the energy target, mid-range of the 20–35% adult
  reference intake.
- **Carbohydrate:** the rest of the energy (4 kcal per gram of protein or
  carbohydrate, 9 per gram of fat), so the three add up to the target.

On a day with recorded steps or sport, the energy that activity adds to the
calorie target (see [Activity](#activity)) is added to that day's
carbohydrate goal, the main fuel of exercise; protein and fat stay the same
every day. Goals you enter yourself are never adjusted. As with the calorie
target, the estimate is recalculated only when you save the goal form, it
uses the same goal rules for pregnancy, breastfeeding or a medical condition,
and Korpus gives no estimate for people under 18.

## Activity

The Activity tab has two tabs, Sport and Gym (see below). Sport records
what you did on a day, entered by hand:

- **Steps.** Record the steps you walked, for example at the end of the
  day. The ring fills at your daily step goal (10,000 unless you change it
  in Settings → Activity). A day without recorded steps shows 0; nothing is
  assumed.
- **Sport.** Pick the sport from the list (the workout types of Apple
  Health) or choose *Custom* to name your own and say how hard it is (easy,
  moderate or hard), then enter how many minutes it lasted and an optional
  note. A session happens once, or repeats every week on chosen days from
  its first day, optionally until an end date. Editing a repeating activity
  changes every day it repeats on, past days included. Set an end date to
  stop it from a given day, or skip a single day from its edit page.

Recorded steps are turned into an approximate energy estimate that the
diary adds to **that day's** calorie target (only when a target is set):

- distance = steps × 0.414 × height (a common stride estimate);
- energy = 0.5 kcal × body weight (kg) × distance (km), the net cost of
  level walking above rest from the ACSM walking equation.

For example, 10,000 steps at 175 cm and 75 kg are about 7.2 km and 270 kcal.
Height comes from Goals; weight is the last weigh-in on or before that day
(otherwise the weight in Goals), so later weigh-ins never change past days.
Without a height and a weight, no estimate is made. Pace, hills and
individual differences are ignored, so treat the number as a rough guide.
The activity level in Goals already accounts for everyday movement: if you
record your steps, choose *Sedentary* there so walking is not counted twice.

Sport sessions are estimated the same way and added to the same target:

- each listed sport has a MET value (energy use as a multiple of resting)
  from the general or moderate-effort entry of the Compendium of Physical
  Activities; a custom sport uses 2.5, 4.5 or 7.5 METs for easy, moderate
  or hard;
- energy = (METs − 1) × body weight (kg) × hours, counting only what is
  spent above rest, which the target already includes.

For example, 30 minutes of running (9 METs) at 75 kg is about 300 kcal. The
weight is chosen as for steps. If the step count you enter already includes
a walk or run you also record as a sport, it is counted twice. Sessions
recorded before the list existed get no estimate until you edit them and
choose a sport.
Activity data is included in the JSON export and the database backup.

## Gym

The Gym tab, under Activity, holds your training programs. A program is made of routines
(such as Push, Pull and Legs), and each routine lists its exercises with the
number of sets, the reps (a number such as 8 or a range such as 8-12), an
optional weight and rest time, and a note. Exercises held for a time, such as
a plank, take a time per set instead of reps. Exercises can be reordered.

- **Schedule.** A program either puts each routine on days of the week, or is
  a *rotation*: the routines follow each other in order (A, B, C, A, …)
  whatever the day. The Gym page shows the current program's week, with
  today marked, or its order.
- **Templates.** Push/Pull/Legs, Upper/Lower, Full body and 5×5 load as
  ordinary programs, named in the interface language, to rename and adapt.
- **Exercises.** About 130 common free-weight, cable, machine and body-weight
  exercises are built in, with names written for Korpus in English, French and
  Spanish using the terms gyms use in each language. Search matches the names
  in all three languages, so "bench press" also finds "Développé couché". An
  exercise that is missing can be added with its muscle group and equipment;
  it cannot be deleted while a routine uses it.
- **Units.** Weights are stored in kilograms and shown in the unit system of
  your profile.

- **Workouts.** Start a workout from today's routine (or the next one of a
  rotation) on the Gym page, from any routine, or empty. Each set starts with
  the numbers you did last time, else the plan, and shows last time's numbers
  beside it. Tick a set when it is done; ticking starts the exercise's rest
  timer. Values are saved as you type, so you can leave the page and come
  back. Sets left unticked are dropped when you finish. When every planned set
  reached the top of its rep range last time, the workout suggests adding a
  little weight; nothing is changed for you. One workout can be in progress at
  a time.
- **History.** Finished workouts list their sets, volume (weight × reps) and
  duration, and can be corrected or deleted later. The routine's plan is
  copied into each workout, so editing or deleting a routine never changes
  past workouts.

Gym programs and workouts are included in the JSON export and the database
backup. The idea and the four templates
follow [openGym](https://github.com/DuarteSantos8/openGym); no code, exercise
text, translations or media were copied from it.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

The tests use real migrations on in-memory and temporary SQLite databases,
and a fake provider with saved Open Food Facts responses in `tests/fixtures`.
They never call live APIs. Importer tests use rows copied from each
publisher's real download.

Code layout (`src/`):

| Module | Responsibility |
|---|---|
| `config` | Environment configuration and validation |
| `db` | Connection setup (WAL, foreign keys) and migrations |
| `nutrition` | Bases, nutrient scaling, kJ/kcal, validation, totals, display rounding |
| `foods` | Food model, manual-entry validation, normalized local search, persistence |
| `providers` | Provider trait, Open Food Facts adapter, TTL cache, rate limiter |
| `datasets` | Food composition tables: importers per publisher format, storage, country preference, local search |
| `countries` | Country list (names in each interface language) |
| `diary` | Meals, entries with nutrition snapshots, daily totals |
| `weight` | Weight history (canonical kg) |
| `goals` | Mifflin–St Jeor estimate, macronutrient split, safety rules, goal settings |
| `habits` | Usual activity asked at first run and the activity level it suggests |
| `gym` | Programs, routines and their exercises; the built-in exercise list (`gym::exercises`), templates (`gym::starter`) and logged workouts (`gym::workouts`) |
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
