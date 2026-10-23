# Korpus --- Claude Code Project Guide

## 1. Product vision

Korpus is a lightweight, self-hostable web application for daily calorie
and macronutrient tracking. It is an approachable alternative to Yazio:
quick to use, fast, private, and easy to run on your own server.

The core loop must be excellent:

1.  Find a food or packaged product.
2.  Verify its nutritional values.
3.  Choose the quantity eaten and add it to a meal.
4.  See daily calories and macros update immediately.
5.  Track weight over time and follow a configurable calorie target.

When a food cannot be found, users must be able to create it manually
and enter its nutritional values. Never block logging because an
external provider is unavailable or a product is missing.

## 2. Non-negotiable principles

-   **Self-hostable:** a user can run Korpus on their own hardware with
    documented, reproducible steps.
-   **Fast:** prioritize quick page loads, responsive search, small
    payloads, and efficient database queries.
-   **Simple:** build the smallest useful feature set first; avoid
    needless abstractions and dependencies.
-   **Web-first:** usable from a normal browser on desktop and mobile. A
    responsive PWA may be added without requiring app stores.
-   **Privacy-first:** personal food logs, weight, and profile data stay
    on the user's installation. No analytics or external tracking by
    default.
-   **Manual-first food logging:** search, barcode lookup where
    supported, quantity entry, and manual food creation are the intended
    workflows.
-   **No AI food recognition:** do not implement image-based meal
    recognition, vision models, photo-to-food estimation, or
    AI-generated macro estimates. Do not add these as optional features
    unless the project owner explicitly changes this requirement.
-   **Transparent nutrition data:** show the data source and whether
    values are complete or missing. Never silently invent nutritional
    values.
-   **Accessible:** target WCAG 2.2 AA; support keyboard navigation,
    visible focus, semantic HTML, useful labels, sufficient contrast,
    and reduced motion.
-   **Maintainable Rust:** use clear module boundaries, explicit error
    handling, tests for calculations, and few dependencies.

## 3. Scope and priorities

### MVP --- implement in this order

1.  Project setup, configuration, database migrations, health endpoint,
    and deployment.
2.  User/profile setup suitable for the deployment model (single-user
    self-hosting first; multi-user support only if the existing
    architecture calls for it).
3.  Food search across configured data sources and the local database.
4.  Food detail view with nutrition per 100 g / 100 ml and source
    attribution.
5.  Manual food creation and editing.
6.  Food diary: dates, meals, serving quantity, add/edit/delete entries,
    and daily totals.
7.  Weight log and weight history chart.
8.  Goal setup and daily calorie target.
9.  Settings, backup/export, import where practical, and complete
    self-hosting documentation.

### Later, only when the MVP is reliable

-   Barcode input/scanning, with manual barcode entry as a fallback.
-   Favorite foods and recent foods.
-   Saved meals / recipes with ingredient-based nutrient totals.
-   Copy a previous day's meals.
-   CSV export/import.
-   Optional additional food database providers.
-   Optional installable PWA support.

Do not build social features, gamification, coaching chatbots,
meal-photo recognition, subscriptions, or complex analytics unless
explicitly requested.

## 4. Food search and nutrition data

Finding food is a central product requirement. Optimize for high
coverage while making source limitations visible.

### Search strategy

-   Search local user-created foods first or alongside external
    providers.
-   Use a provider abstraction so data sources can be added or replaced
    without rewriting diary logic.
-   Start with Open Food Facts for branded/barcoded products and
    evaluate a suitable generic-food dataset/provider (for example, USDA
    FoodData Central where appropriate for the deployment and licensing
    requirements).
-   Do not assume one source covers every product or region. Support
    provider configuration and graceful degradation.
-   Support exact barcode lookup when a barcode is available, followed
    by text search.
-   Search should be debounced, paginated, bounded, and
    rate-limit-aware. Do not issue an external request for every
    keystroke.
-   Use normalized search terms and rank exact barcode/name matches
    above partial matches. Deduplicate results across providers when
    identifiers and data support it.
-   Prefer provider APIs and documented endpoints. Respect rate limits,
    attribution, API terms, and dataset licenses. Do not scrape sites
    when a supported API exists.
-   Configure an honest, identifying User-Agent for external requests
    where required.
-   Cache suitable provider responses locally with clear expiry and
    error behavior. Never make external availability a prerequisite for
    viewing or editing locally saved foods.
-   Do not promise complete global product coverage. Make the system
    extensible and show when no result is found.

### Result and detail requirements

Each result should display, when available: - Food/product name and
brand. - Barcode or provider identifier. - Energy (kcal), protein,
carbohydrate, and fat per 100 g or 100 ml. - A clear indicator when key
nutrition fields are missing. - Data source and, if available, the
last-updated date.

Display fiber, sugars, saturated fat, salt, and serving information when
supplied, but do not make them mandatory for a basic food entry.

Treat nutrition data as untrusted external input. Validate values and
units. Preserve source values and their basis rather than silently
changing them.

### Manual food creation

Users must be able to create a food when search fails. Fields: - Name
(required). - Brand (optional). - Barcode (optional; unique when
supplied where possible). - Basis: per 100 g or per 100 ml (make the
basis explicit). - Energy in kcal, protein in grams, carbohydrates in
grams, and fat in grams. - Optional fiber, sugars, saturated fat, salt,
and notes. - Source set to user-created.

Allow missing optional nutrients. Validate that values are finite and
non-negative. Do not force macros to add up to a fixed percentage or
silently "correct" user-entered data. Permit editing and deletion with
clear handling for diary entries that already reference a food.

### Quantity and calculations

-   Store food nutrition on a consistent, explicit basis (normally per
    100 g or per 100 ml).
-   Diary entries store the amount consumed and its unit/basis.
-   For gram-based foods:
    `entry_nutrient = nutrient_per_100g * amount_g / 100`.
-   For milliliter-based foods: use per-100-ml values and
    `amount_ml / 100`.
-   Do not assume grams equal milliliters or convert between them
    without a reliable density.
-   Use decimal-safe storage/calculation where practical; avoid repeated
    floating-point rounding. Round for display, not after every
    intermediate operation.
-   Energy values from providers may be expressed in kJ or kcal. Convert
    explicitly when needed (`1 kcal = 4.184 kJ`) and record/display the
    chosen unit correctly.
-   Daily totals must be derived from diary entries and their nutrition
    snapshots or a documented equivalent. Editing a food later must not
    unexpectedly rewrite historical diary totals. Prefer storing a
    nutrition snapshot on each logged entry so past records remain
    stable.

## 5. Diary and daily dashboard

The daily view is the primary screen. Keep it scannable and fast.

It should show: - Selected date and easy previous/next-day navigation. -
Daily calorie target, calories consumed, and remaining calories. -
Protein, carbohydrate, and fat consumed versus their goals, where
configured. - Meals (at minimum breakfast, lunch, dinner, and snacks),
each with entries and subtotals. - A prominent, quick "Add food" flow. -
Useful empty states and clear loading/error states.

Users can add, edit, and delete an entry; choose meal, food, amount, and
date; and log the same food more than once. Make common actions require
as few interactions as possible without hiding important quantity or
nutrition details.

Daily totals should update consistently after mutations. Handle time
zones and dates deliberately; a diary date is a calendar date, not an
arbitrary server timestamp.

## 6. Weight and calorie goals

Users can record weight with a date and optional note. Show a simple
trend chart and a history list; allow corrections and deletion. Store
units explicitly and support at least kg and lb, converting only for
display or through a well-defined canonical representation.

### Calorie target workflow

Collect only the information needed by the selected calculation: units,
height, current weight, age, sex parameter required by the chosen
equation, activity level, and goal (maintain, lose, or gain). Explain
why each field is requested and allow users to change or skip optional
profile information where possible.

-   Use a documented, conventional estimate for resting energy
    expenditure (for example, Mifflin--St Jeor) and a clearly explained
    activity multiplier to estimate maintenance calories.
-   Apply a configurable, conservative deficit or surplus to suggest a
    daily target.
-   Show the estimate, assumptions, and chosen goal; let users set a
    custom target.
-   Recalculate only when relevant profile inputs or goal settings
    change. Do not silently change a target every time the scale
    changes.
-   Do not claim that the result is exact or medically individualized.
    Include a concise disclaimer that calorie estimates are approximate
    and not medical advice.
-   Do not encourage extreme restriction, rapid weight change, or unsafe
    targets. Use conservative limits and clear validation; if the user
    is under 18, pregnant, has a relevant medical concern, or enters an
    obviously unsafe target, avoid prescribing a weight-loss target and
    recommend qualified professional guidance.
-   Keep this feature informational, not diagnostic. Avoid moralizing
    language, shame, streak pressure, or labeling foods as "good" or
    "bad".

Weight tracking and calorie targets are personal data. Keep them private
and never transmit them to an external service.

## 7. Architecture and technology

Honor the repository's existing architecture if one exists. Before
making large changes, inspect the project structure, README, dependency
manifest, and tests. Do not replace working technology merely because
another stack is preferred.

For a new implementation, prefer a small, conventional Rust web stack: -
Rust stable. - Axum for HTTP routing and handlers. - Tokio for async
runtime. - SQLx with migrations and compile-time/query validation where
practical. - SQLite as the default single-instance database for easy
self-hosting; keep data access portable enough to consider PostgreSQL
later if multi-user/concurrent deployments require it. - Server-rendered
HTML with a lightweight progressive-enhancement approach (for example,
Askama templates plus small amounts of HTMX or vanilla JavaScript)
unless the repository already uses a frontend framework. Avoid a large
SPA by default. - `tracing` for structured logs. - `serde` for
serialization and `thiserror` or a similarly small error-handling
approach. - A small charting solution or accessible SVG for weight
history; do not add a large frontend framework just for one chart.

These are defaults, not a reason to ignore the existing project. Verify
current library APIs and compatibility when adding dependencies. Pin and
update dependencies deliberately; do not add a crate for functionality
already available in the standard library or existing stack without a
clear benefit.

### Suggested module boundaries

Adapt names to the existing codebase, but keep responsibilities
separated: - `config`: environment/config loading and validation. -
`web` or `routes`: HTTP routes and request/response handling. - `auth`:
authentication/session management if enabled. - `db` / `repositories`:
persistence and migrations. - `foods`: food model, local search, manual
editing, nutrition validation. - `providers`: external food-source
adapters, normalization, caching, rate limits. - `diary`: entries,
meals, daily totals, nutrition snapshots. - `weight`: weight history and
conversions. - `goals`: energy estimate, goal settings, validation. -
`templates` / `static`: accessible UI and minimal client-side behavior.

Keep business logic out of HTTP handlers and templates.
Provider-specific response types must not leak into the core food model.

## 8. Data model guidance

Design migrations before implementing features. A likely starting point
includes: - `users` or a single local profile, depending on the chosen
authentication model. - `profile_settings`: units, height, birth
date/age input strategy, calculation inputs, activity level, and goal
preferences. - `foods`: name, brand, barcode, nutrition basis,
nutrients, source, provider ID, timestamps, and ownership/visibility. -
`diary_entries`: diary date, meal, amount, unit, food reference,
created/updated timestamps, and a nutrition snapshot. -
`weight_entries`: date, weight in canonical unit, optional note,
timestamps. - `goal_settings`: current target, goal type, calculation
inputs/version, and timestamps. - Optional `favorites`, `recipes`, and
`recipe_ingredients` only when those features are implemented.

Add appropriate foreign keys, indexes for common searches and date
queries, uniqueness constraints where justified, and migration
rollback/upgrade guidance. Decide and document whether a local food is
private to its creator or shared within an installation.

Never store passwords in plaintext. If authentication is required, use a
vetted password-hashing library, secure sessions, CSRF protection for
cookie-authenticated mutations, and safe cookie settings. For a
single-user local deployment, do not invent a public registration system
without a requirement; document the security implications of exposing
the service to a network.

## 9. UI and accessibility

-   Mobile-first responsive layout; diary and food search must work well
    on narrow screens.
-   Keep the daily calorie summary visible without making the interface
    feel crowded.
-   Use plain language and consistent metric/imperial unit handling.
-   Every input has a visible label, helpful validation, and an
    accessible error message.
-   All core workflows work with a keyboard and screen reader.
-   Do not use color as the only way to communicate status.
-   Respect reduced-motion settings and avoid unnecessary animation.
-   Provide empty, loading, offline/provider-unavailable, not-found, and
    failure states.
-   Avoid modal dialogs when a simple page or inline panel is clearer.
-   Do not show fake data in production. Clearly mark demo/seed data.
-   Use a consistent locale-aware date and number format; store dates
    and units unambiguously.

## 10. Privacy, security, and self-hosting

Self-hosting should be a first-class experience, not an afterthought.

-   Provide a minimal Dockerfile and `compose.yaml` if suitable for the
    project.
-   Persist the database in a mounted volume; never store important data
    only in the container filesystem.
-   Provide `.env.example` with documented defaults and no real secrets.
-   Bind to a configurable address and port. Document reverse-proxy/TLS
    setup and warn against exposing an unauthenticated instance to the
    public internet.
-   Include health/readiness endpoints suitable for container
    orchestration.
-   Support database backup and restore, with tested instructions.
    SQLite backups must use a safe procedure that accounts for active
    writes.
-   Document upgrades and migration behavior; never destroy user data
    during startup.
-   Keep secrets out of logs and error messages.
-   Use parameterized SQL; validate user input; prevent XSS through
    contextual template escaping; apply CSRF protection where
    appropriate; set secure HTTP headers.
-   Set reasonable request body, query length, and pagination limits.
-   Use timeouts and bounded retries for external providers. A failed
    provider should not crash the app.
-   No telemetry, advertising, third-party tracking scripts, or external
    AI calls by default.
-   Explain every external network dependency and provide a way to
    disable optional providers.
-   Respect upstream dataset licenses, attribution requirements, and API
    terms. Open Food Facts data is open but has specific licensing and
    usage conditions; document them before distributing derived
    datasets.

## 11. Reliability and performance

-   Avoid N+1 database queries and unbounded result sets.
-   Add indexes based on actual queries and use pagination for search
    and history.
-   Keep external API calls out of critical database transactions.
-   Set provider timeouts, cache results where appropriate, and degrade
    gracefully.
-   Do not perform network calls during page rendering when cached/local
    data can be used first.
-   Use structured logs without sensitive food diary, weight,
    credential, or profile details.
-   Return actionable user-facing errors while keeping internal
    diagnostics in logs.
-   Prefer simple, measurable optimizations over speculative caching or
    complicated concurrency.
-   Ensure the app starts reliably with a fresh database and survives
    provider outages.

## 12. Testing requirements

Add tests as features are implemented. At minimum: - Unit tests for
nutrient scaling by quantity, kcal/kJ conversion, unit conversion, daily
totals, and calorie-goal calculations. - Validation tests for negative,
non-finite, missing, and implausible values. - Tests proving historical
diary snapshots remain stable when a food record changes. - Provider
adapter tests using fixtures; do not make normal test suites depend on
live external APIs. - Database/repository tests against a temporary test
database and real migrations. - Route/integration tests for creating
foods, logging entries, changing quantities, deleting entries, and
recording weight. - Tests for date boundaries and timezone-sensitive
diary behavior. - Accessibility checks and manual keyboard testing for
key flows. - Run formatting, linting, and tests before considering a
change complete.

Use `cargo fmt --check`,
`cargo clippy --all-targets --all-features -- -D warnings` where
feasible, and `cargo test`. If the project has a different established
command set, document and use it.

## 13. Developer workflow and code conventions

-   Read relevant code and tests before editing.
-   Make focused, incremental changes; avoid unrelated refactors.
-   Prefer clear, idiomatic Rust and small functions with explicit
    responsibilities.
-   Use typed domain models and validated units rather than ambiguous
    numeric fields.
-   Avoid `.unwrap()` and `.expect()` on runtime/user/provider input
    paths; handle errors explicitly. They may be appropriate in tests or
    proven invariants.
-   Do not silently swallow errors.
-   Keep comments for reasoning and non-obvious constraints, not
    line-by-line narration.
-   Add a dependency only when its benefit is clear; explain significant
    additions.
-   Update migrations, tests, docs, `.env.example`, and deployment files
    whenever behavior or configuration changes.
-   Never claim a command or test passed unless it was actually run.
-   If requirements are ambiguous, choose the simplest safe option
    consistent with this file and state the assumption in the final
    summary.
-   Do not implement speculative features or broaden scope without
    asking.

## 14. Definition of done

A change is complete only when applicable items are satisfied: - It
solves the requested user problem and fits Korpus's product
principles. - Validation and error states are implemented. - It is
accessible and responsive. - Business logic has tests, including
relevant edge cases. - Database migrations are included and safe. -
Documentation/configuration is updated. - No AI recognition or
AI-generated nutrition estimates were introduced. - No user data is sent
to an external service except the explicitly configured provider
requests needed for the feature. - Formatting, linting, and tests have
been run, with failures reported honestly.

## 15. First-run experience

A new self-hosted instance should have a short, clear setup path: 1.
Start the service using documented instructions. 2. Create or configure
the local profile according to the deployment mode. 3. Set units and
optionally configure a calorie goal. 4. Land on today's diary with an
obvious "Add food" action. 5. Find a product, or create one manually if
it is missing.

Do not require an external account, AI provider, or cloud service to use
the core diary. The app must remain useful when every optional external
food provider is unavailable.
