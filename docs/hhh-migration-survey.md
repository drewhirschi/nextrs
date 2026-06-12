# hhh-next → nextrs Conversion Worksheet

Survey of `/home/drew/work/hhh-next` (Next.js 16 App Router, bun, better-auth, postgres-js, S3/MinIO, sharp, shadcn/radix, Tailwind 4). Goal: keep the React frontend pixel-identical; replace the entire Node backend with a single Axum binary using nextrs conventions (`page.tsx` + `props.rs` server seeds, `route.rs` API handlers).

**Headline finding:** this app's backend surface is almost entirely **React Server Actions** — 12 `'use server'` modules exporting **68 action functions**, called directly from client components. There are only **2 conventional API route files** (`/api/auth/[...all]` and `/api/avatar`). The conversion is therefore dominated by two problems: (1) replacing the server-action RPC mechanism with explicit HTTP endpoints + a client shim, and (2) reproducing better-auth's wire protocol in Rust (or keeping it as a sidecar).

---

## 1. Page inventory (23 `page.tsx` files)

Legend: **RSC** = server component, **CC** = `'use client'`. "Server data" = data fetched during server render. All `/admin` and `/app` auth gating happens in `proxy.ts` (middleware), not in pages.

### Public + auth

| Route | Type | Server data | Layout chain | Loading | Dynamic seg | Guards / redirects |
|---|---|---|---|---|---|---|
| `/` | CC | none | root | – | – | proxy redirects authed users to `/admin` or `/app`; page also does client-side `useSession` redirect (belt & suspenders) |
| `/about` | RSC (static, no data) | none | root | – | – | none |
| `/contact` | RSC (static, no data) | none | root | – | – | none |
| `/pricing` | RSC (static, no data) | none | root | – | – | none |
| `/auth/login` | CC | none | root | – | – | proxy redirects authed users away; calls `signIn.email`, `signIn.social({provider:'google'})`; honors `?redirect=` param; wraps itself in `Suspense` (uses `useSearchParams`) |
| `/auth/register` | CC | none | root | – | – | proxy redirect; calls `signUp.email`, `signIn.social` |

### Admin (`/admin/*` — proxy requires session + role ADMIN)

| Route | Type | Server data | Loading | Dynamic seg | Actions called (client-side unless noted) |
|---|---|---|---|---|---|
| `/admin` | **RSC**, `export const dynamic = 'force-dynamic'` | `getDashboardStats()`, `getUpcomingInstances({days:7})` awaited in the server component, passed as props to `AdminDashboardClient` | `admin/loading.tsx` (spinner) | – | dashboard-client.tsx calls `checkInBooking`, `uncheckInBooking` |
| `/admin/bookings` | CC | none | `loading.tsx` | – | `getAllBookings`, `cancelBookingAdmin`, `checkInBooking`, `uncheckInBooking` |
| `/admin/classes` | **RSC** | `getClassTemplates()` awaited server-side, rendered directly | `loading.tsx` | – | – |
| `/admin/classes/new` | CC | none | (inherits) | – | `createClassTemplate` |
| `/admin/classes/[id]` | CC | none (loads via action in `useEffect`, `use(params)`) | (inherits) | `[id]` | `getClassTemplate`, `updateClassTemplate`, `deleteClassTemplate`; calls `notFound()` client-side |
| `/admin/customers` | CC | none | `loading.tsx` | – | `getAllCustomers`, `getCustomerStats`, `recordExternalPayment`, `getActiveProductsForPayment`; uses `useSearchParams` (Suspense-wrapped) |
| `/admin/customers/new` | CC | none | (inherits) | – | `getAvailableUsers`, `createUserAndInvite`, `createCustomerWithProduct`, `getActiveProducts` |
| `/admin/customers/[id]` | CC | none | (inherits) | `[id]` | `getCustomerWithUser`, `updateCustomer`, `cancelCustomer`, `getPaymentsByCustomer`, `recordExternalPayment`, `getActiveProductsForPayment`, `getCustomerProducts`, `attachProductToCustomer`, `updateCustomerProduct`, `cancelCustomerProduct`, `getSubscriptionByCustomer`, `updateBillingPeriod`; **also raw `fetch('/api/avatar', POST/DELETE ?userId=)`** for admin avatar management |
| `/admin/instances/[id]` | CC | none | (inherits) | `[id]` | `getClassInstanceWithBookings`, `updateClassInstance`, `deleteClassInstance`, `checkInBooking`, `uncheckInBooking`, `createBookingAdmin`, `cancelBookingAdmin`, `searchUsers`; client `notFound()` |
| `/admin/products` | CC | none | `loading.tsx` | – | `getAllProducts`, `deactivateProduct` |
| `/admin/products/new` | CC | none | (inherits) | – | `createProduct` |
| `/admin/products/[id]` | CC | none | (inherits) | `[id]` | `getProduct`, `updateProduct`, `deactivateProduct`, `activateProduct` |
| `/admin/schedule` | CC | none | `loading.tsx` | – | `getScheduleSlots`, `createScheduleSlot`, `deleteScheduleSlot`, `updateScheduleSlot`, `getClassTemplates`, `fillScheduleForWeek`, `getInstancesForWeek`, `createOneOffInstance` (uses `ScheduleCalendar` + dnd-kit) |

### User app (`/app/*` — proxy requires session; admins bounced to `/admin` unless `?view=user`)

| Route | Type | Server data | Loading | Actions / fetches |
|---|---|---|---|---|
| `/app` | CC | none | `app/loading.tsx` | `getAvailableClasses`, `bookClass`, `getBookingAllowance` (userId from `useSession`) |
| `/app/my-bookings` | CC | none | `loading.tsx` | `getUserBookings`, `cancelBooking` |
| `/app/profile` | CC | none | `loading.tsx` | `signOut()`; avatar flow: `fetch('/api/avatar')` GET (presign) → `fetch(presignedUrl, PUT)` direct to S3 → `fetch('/api/avatar', POST json)` (process) → `refetch()` session; `fetch('/api/avatar', DELETE)` |
| `/app/subscription` | CC | none | `loading.tsx` | `getCustomerByUserId`, `getSubscriptionByCustomer`, `getPaymentsByCustomer`, `getBookingAllowance` |

**Page conversion takeaway:** only **2 pages need `props.rs` server seeds** (`/admin`, `/admin/classes`). 3 are static RSC with no data (render as-is). The other 18 are pure client components that fetch everything post-mount — they need zero page-level backend work beyond the action endpoints existing.

---

## 2. Layouts, middleware, config

### Layouts
- **`app/layout.tsx` (root)** — server component but **no data fetching**. Sets `Metadata` (title, PWA manifest `/manifest.json`, apple-web-app, icons) and `Viewport` (theme color, safe-area). Renders `<Toaster/>` (sonner) and imports `styles.css`. nextrs needs: equivalent `<head>` output (manifest link, viewport meta, apple meta tags, favicon/apple-touch-icon links).
- **`app/admin/layout.tsx`** — `'use client'`. Pure UI (header + tab nav via `usePathname`). No server work. Keep as-is.
- **`app/app/layout.tsx`** — `'use client'`. Bottom mobile nav; reads `useSession()` client-side to show the admin "Viewing as user" banner; `useSearchParams` for `?view=user` (Suspense-wrapped). No server work. Keep as-is.
- **`loading.tsx` ×10** (recounted) (`admin`, `admin/bookings`, `admin/classes`, `admin/customers`, `admin/products`, `admin/schedule`, `app`, `app/my-bookings`, `app/profile`, `app/subscription`) — all identical Loader2 spinners. Only matter for the 2 RSC pages; nextrs streaming/loading parity decision needed (or render seed fast enough to skip).
- **`app/error.tsx`** (client error boundary, shows `error.message` + reset) and **`app/not-found.tsx`** (static 404). Need nextrs equivalents (404 handler; error boundary stays client-side React).

### `proxy.ts` (root)
This is **Next 16's renamed `middleware.ts`** (Next 16 renamed the file/export to `proxy`). It runs on every request matching its matcher and:
1. Calls its **own** `/api/auth/get-session` via `betterFetch`, forwarding the `cookie` header.
2. Guards: `/app/*` → must be authed (else redirect `/auth/login?redirect=<path>`); admins hitting `/app` get redirected to `/admin` unless `?view=user`. `/admin/*` → must be authed AND `role === 'ADMIN'` (else `/app`). `/auth/*` and `/` → authed users redirected to `/admin` or `/app` by role.
3. Matcher excludes `api/auth`, `_next/static`, `_next/image`, `favicon.ico`, `manifest.json`, `robots.txt`, `icon-*`, `apple-touch-icon.png`, `hhh_logo.png`.

**nextrs mapping:** an Axum middleware/extractor layer that resolves the session once per request (in-process — no HTTP self-call needed) and issues 307 redirects with the same rules and the same exclusion list.

### `next.config.ts`
- `reactStrictMode: true`
- `experimental.serverActions.bodySizeLimit: '2mb'` → the replacement action endpoints should accept ≥2 MB bodies.
- `images.remotePatterns: https://**` — irrelevant in practice; **`next/image` is never used anywhere** (all avatars are plain `<img>`). No parity work needed.
- No rewrites, redirects, headers, i18n, output mode.

### `vercel.json`
Vercel deploy with bun (`bunVersion: 1.x`, `buildCommand: bun run build` which runs `db:migrate` first, then `next build`). Note: migrations run at **build** time today. nextrs deploy should run migrations at startup or as a deploy step.

---

## 3. API routes (2 files)

### `app/api/auth/[...all]/route.ts`
`export const { GET, POST } = toNextJsHandler(auth.handler)` — the entire better-auth HTTP surface mounted at `/api/auth/*`. See §4 for the endpoints actually exercised.

### `app/api/avatar/route.ts` — GET / POST / DELETE
All three methods: resolve session from request headers via `AuthCore.getSessionFromHeaders` (better-auth `auth.api.getSession`); 401 if unauthenticated; optional `userId` (query param for GET/DELETE, JSON or form field for POST) lets an **ADMIN** act on another user (403 for non-admin).

| Method | Request | Response | Backend work |
|---|---|---|---|
| GET | `?userId=` optional | `{ uploadUrl, userId }` | presigned **PUT** URL (5 min expiry) for `avatars/{userId}/temp-upload` in the avatar bucket |
| POST (json) | `{ userId? }` | `{ url, thumbUrl }` | fetch `temp-upload` from S3 → **sharp**: auto-rotate (EXIF) + cover-resize to 256 & 64 → webp q80 → write `avatar.webp` + `avatar-thumb.webp` → delete temp → `UPDATE "user" SET image = <publicUrl>?v=<ts>` |
| POST (form-data, legacy) | `avatar` File + `userId?` | `{ url, thumbUrl }` | same pipeline from the uploaded bytes; validates type (jpeg/png/webp/gif) and ≤4 MB |
| DELETE | `?userId=` optional | `{ success: true }` | delete both S3 objects, `UPDATE "user" SET image = NULL` |

Errors: `{ error: string }` with 400/401/403/413/500. **No zod on this route** (manual validation).

### The 68 server actions (the real API surface)

All actions take `unknown` input, validate with **zod** (`schema.parse` — throws on bad input), and return plain serialized data (or throw; `user-bookings.ts` instead returns `ActionResult<T> = { success, data } | { success: false, errorKey, errorMessage }` with stable error keys: `CLASS_NOT_FOUND`, `CLASS_FULL`, `ADVANCE_BOOKING_LIMIT`, `NO_SUBSCRIPTION`, `BOOKING_LIMIT_REACHED`, `ALREADY_BOOKED`, `BOOKING_NOT_FOUND`, `UNKNOWN`).

**⚠ Authorization gap to preserve-or-fix consciously:** the actions themselves do **no session checks** (except implicitly via proxy guarding the pages that call them). As Next server actions they are public POST endpoints; in nextrs you should put admin-actions behind an admin guard at the router level (`/api/admin/*`) — this is a behavior *improvement*, decide explicitly.

| Module | Actions (zod schema → core fn) | Notes |
|---|---|---|
| `actions/auth.ts` | `validateSession()` (reads request headers — **unused by any page**, can drop), `searchUsers({query,limit≤20})` | searchUsers does ILIKE on name/email |
| `actions/bookings.ts` | `getAllBookings(filter{status?,from?,to?})`, `cancelBookingAdmin(id)`, `checkInBooking(id)`, `uncheckInBooking(id)`, `getBookingsForClass(instanceId)`, `createBookingAdmin({userId,classInstanceId})` | admin booking mgmt |
| `actions/class-instances.ts` | `getUpcomingInstances({days=7}?)`, `createOneOffInstance(...)`, `deleteClassInstance(id)`, `getClassInstanceWithBookings(id)`, `fillScheduleForWeek({weekStartDate})`, `getInstancesForWeek({weekStartDate})`, `updateClassInstance(...)` | the 4 mutating ones call `revalidatePath('/admin')` — see §8 |
| `actions/class-templates.ts` | `getClassTemplates()`, `getClassTemplate(id)`, `createClassTemplate({name,description?,duration,capacity,category?})`, `updateClassTemplate({id,data})`, `deleteClassTemplate(id)` | |
| `actions/customer-products.ts` | `attachProductToCustomer({customerId,productId,customPrice?,expiresAt?})`, `getCustomerProducts(customerId)`, `getActiveCustomerProducts(customerId)`, `updateCustomerProduct({id,customPrice?,status?,expiresAt?})`, `setCustomerProductPrice({id,customPrice})`, `cancelCustomerProduct(id)`, `customerHasProduct({customerId,productId})` | tri-state `expiresAt` (undefined / null / date) — careful in Rust |
| `actions/customers.ts` | `getAllCustomers(filter)`, `getCustomer(id)`, `getCustomerByUserId(userId)`, `createCustomer(...)`, `updateCustomer({id,data})`, `cancelCustomer(id)`, `getCustomerStats()`, `getAvailableUsers()`, `createUserAndInvite({name,email})`, `createCustomerWithProduct({userId,notes?,billingType,productId?,customPrice?})`, `getCustomerWithUser(customerId)` | `createUserAndInvite` calls **`auth.api.createUser`** (better-auth admin plugin, server API) with a random temp password, then POSTs to its own `/api/auth/forget-password` to trigger reset email (which today only `console.log`s). `createCustomerWithProduct` does a 3–4 statement multi-insert **without a transaction** |
| `actions/dashboard.ts` | `getDashboardStats()` → `{totalTemplates, upcomingClasses, bookingsNext24h}` | 3 parallel counts |
| `actions/payments.ts` | `getAllPayments(filter)`, `getPaymentsByCustomer(id)`, `recordExternalPayment({customerId,amount,method,billingPeriod...,productId?,notes?})`, `getPaymentStats()`, `getActiveProductsForPayment()` | stats computes month bounds server-side |
| `actions/products.ts` | `getAllProducts()`, `getActiveProducts()`, `getProduct(id)`, `createProduct({name,type:recurring\|pack,price,credits?,bookingLimitType,bookingLimitValue?...})`, `updateProduct({id,data})`, `deactivateProduct(id)`, `activateProduct(id)` | |
| `actions/schedule-slots.ts` | `getScheduleSlots()`, `createScheduleSlot({templateId,dayOfWeek 0-6,time "HH:MM"})`, `deleteScheduleSlot(id)`, `updateScheduleSlot({id,...})` | |
| `actions/subscriptions.ts` | `getSubscriptionByCustomer(id)`, `createSubscription(...)`, `updateSubscription({id,data})`, `pauseSubscription(id)`, `resumeSubscription(id)`, `cancelSubscription(id)`, `getSubscriptionStats()`, `updateBillingPeriod({customerId,periodStart,periodEnd})` | several unused by UI (create/pause/resume/cancel only partially wired) |
| `actions/user-bookings.ts` | `getAvailableClasses({userId?})`, `bookClass({userId,classInstanceId})`, `getUserBookings(userId)`, `cancelBooking({bookingId,userId})`, `getBookingAllowance(userId)` | returns `ActionResult<T>`; `getUserBookings` remaps `class_instance` → `classInstance` for frontend compat. **userId is taken from client input, not session** — trusts the caller (same authz gap as above) |

---

## 4. Auth (better-auth 1.4)

**Config (`src/lib/auth.ts`):**
- Database: kysely via `PostgresJSDialect` over a dedicated `postgres()` connection (better-auth manages its own queries against `user`/`session`/`account`/`verification`).
- **Field mapping: every table uses snake_case columns** (explicit `fields` maps for user/session/account/verification). Tables are app-managed in `migrations/001`.
- `emailAndPassword: enabled, minPasswordLength 7` — password hash is **better-auth's default scrypt**, stored in `account.password` (provider `credential`). A Rust replacement must verify these scrypt hashes to keep existing users.
- `socialProviders.google` (`GOOGLE_CLIENT_ID/SECRET`) — full OAuth code flow: `/api/auth/sign-in/social` → Google → `/api/auth/callback/google`. `state` rows stored in `verification`.
- `sendResetPassword` — **stub: console.logs the URL** (no real email; `RESEND_API_KEY` env exists but resend is never imported).
- Plugins: `admin({ defaultRole: 'USER', adminRoles: ['ADMIN'] })` — adds `role`, `banned`, `ban_reason`, `ban_expires` to user and `impersonated_by` to session (migration `010_admin_plugin.sql`); exposes `auth.api.createUser` (used by `createUserAndInvite`).
- `user.additionalFields.role` (string, default 'USER') — so `session.user.role` is returned by `get-session`. **The client depends on `role` being present in the session payload** (proxy, layouts, landing page).
- Session strategy: **database sessions** (row in `session`, token in cookie). Default cookie: **`better-auth.session_token`** (`__Secure-better-auth.session_token` on HTTPS), 7-day expiry with rolling refresh (better-auth defaults; nothing overridden). No cookie-cache enabled.

**Endpoints the frontend actually hits at runtime** (via `createAuthClient` with `baseURL = NEXT_PUBLIC_BETTER_AUTH_URL`, baked into the client bundle):
1. `GET /api/auth/get-session` — `useSession()` (used in proxy, `/`, `/app/*` layout+pages, login/register) — response must include `user.{id,email,name,image,role}`.
2. `POST /api/auth/sign-in/email` `{email,password}` — sets session cookie.
3. `POST /api/auth/sign-up/email` `{name,email,password}`.
4. `POST /api/auth/sign-in/social` `{provider:'google', callbackURL}` → returns redirect URL; then `GET /api/auth/callback/google`.
5. `POST /api/auth/sign-out`.
6. `POST /api/auth/forget-password` — called **server-to-self** by `createUserAndInvite` (and would be used by a future reset UI).
7. `adminClient()` plugin is loaded client-side but **no `authClient.admin.*` calls exist in the UI** — only the server-side `auth.api.createUser`.

**Server-side session checks:** `proxy.ts` (HTTP self-call) and `AuthCore.getSessionFromHeaders` (in-process, used by `/api/avatar` and the unused `validateSession`). Nothing else checks sessions.

**nextrs options (decision required):**
- (a) Reimplement the 6 endpoints in Rust: cookie format, token lookup w/ rolling expiry, scrypt verify, Google OAuth code flow, snake_case tables. Highest effort, single binary.
- (b) Keep better-auth as a tiny Node/Bun sidecar mounted at `/api/auth/*` behind the Axum reverse-proxy route; Rust validates sessions by reading the `session` table directly (cookie token → DB row → join user). Pragmatic; preserves hashes/OAuth exactly. Rust-side session check is trivial either way since sessions are DB rows.

---

## 5. Data layer

**Connection setup:** `src/lib/db.ts` — single `postgres(process.env.DATABASE_URL)` client (postgres-js tagged templates; **not kysely** — kysely only exists as better-auth's dialect). Scripts (`migrate/reset/seed`) use **Bun's built-in `sql`** instead. `db-types.ts` (257 lines) is hand-kept TS row types; `schema/schema.prisma` is an **auto-generated snapshot** (by `scripts/schema-gen.ts` after migrate) — not a Prisma runtime, just documentation. nextrs equivalent: sqlx/deadpool-postgres pool from `DATABASE_URL`.

**Migrations:** 14 ordered `.sql` files in `migrations/`, applied by `scripts/migrate.ts` into a `schema_migrations(id, applied_at, checksum)` table, each inside a transaction (BEGIN/COMMIT stripped from file, re-wrapped). A Rust migration runner must be **compatible with the existing `schema_migrations` rows** (same ids `001_initial_schema` etc.) so prod doesn't re-apply.

**Tables (15 incl. bookkeeping):**

| Table | Purpose / key columns |
|---|---|
| `user` | better-auth + `role` (USER/ADMIN), `image`, admin-plugin cols (`banned`, `ban_reason`, `ban_expires`) |
| `session` | token (unique), expires_at, user_id, `impersonated_by` |
| `account` | provider rows; `password` (scrypt) for credential provider |
| `verification` | OAuth state / reset tokens |
| `class_template` | name (unique), description, duration, capacity, `category` (006); day/time/is_recurring **dropped** in 013 |
| `class_instance` | template_id (SET NULL), name, date, time, duration, capacity, `starts_at TIMESTAMPTZ` (004), `category` (006), unique(template_id, starts_at) (003) |
| `booking` | user_id, class_instance_id, status CONFIRMED/CANCELLED, `checked_in_at` (007), **UNIQUE(user_id, class_instance_id)** (012) |
| `schedule_slot` | template_id, day_of_week 0–6, time "HH:MM", UNIQUE(template,dow,time) (014) |
| `customer` | user_id (unique), monthly_rate DECIMAL (legacy; rate now derived from products), status, notes, signed_up_at |
| `subscription` | customer_id (unique), billing_type STRIPE/EXTERNAL, status, stripe ids (unused — no Stripe integration exists), current_period_start/end DATE |
| `payment` | customer_id, subscription_id?, amount DECIMAL, status, payment_method, billing_period_start/end, `product_id` (008-era) |
| `product` | name, type `recurring`/`pack`, price DECIMAL, credits?, `booking_limit_type` unlimited/per_week/per_billing_period + value (011), active |
| `customer_product` | customer_id, product_id, status ACTIVE/EXPIRED/CANCELLED, `custom_price` (009), purchased_at, expires_at |
| `user_credits` | customer_id, customer_product_id?, credits_remaining, FIFO by created_at |
| `schema_migrations` | migration bookkeeping |

**Query patterns to reproduce:**
- postgres-js tagged-template SQL throughout core modules; heavy use of `json_build_object` / nested `json_agg` subqueries to return joined shapes (customer+user, booking+user+class_instance, instance+bookings[]+user). The JSON-column trick maps nicely to Postgres-side JSON in Rust too (select as `serde_json::Value` or typed structs).
- `COALESCE(SUM(COALESCE(cp.custom_price, p.price)))::float` derived monthly rate (in `customers.ts` action and `customer/index.ts`).
- Interval filters: `starts_at >= NOW() AND starts_at <= NOW() + (days || ' days')::interval` style; month-bounds revenue aggregations grouped by payment_method.
- **Transactions: none in app code.** The only `sql.begin` is in the migration runner. Two latent concurrency issues worth fixing during port: `user_credits.deductCredit` issues `SELECT ... FOR UPDATE` **outside any transaction** (the lock is released immediately — ineffective), and `bookClass` does check-capacity-then-insert without a transaction (race to overbook; partially mitigated by UNIQUE(user_id,class_instance_id)). The Rust version should wrap book/cancel/credit flows in real transactions.
- `crypto.randomUUID()` TEXT primary keys generated app-side; timestamps written as ISO strings.
- **Serialization caveat:** DECIMAL columns come back from postgres-js as **strings** unless cast (code casts revenue sums to `::float`; `price`, `amount`, `monthly_rate` row fields arrive as strings and the UI tolerates it). Dates arrive as JS `Date` objects and survive the server-action RPC as Dates; over plain JSON they become ISO strings — see §7 risk.

**Business logic hotspot — `src/lib/core/booking` (650 lines, well-tested):** `checkBookingEligibility` (active subscription OR credits), `checkBookingLimit` (product `booking_limit_type`: unlimited / per_week (week bucketing) / per_billing_period (uses subscription period dates); falls back to credits), `getBookingAllowance` (remaining-classes UI), `bookClass` (capacity check, 7-day advance window `isWithinBookingWindow`, reactivate-cancelled-booking path, credit deduction), `cancelByUser` (credit refund logic). Plus `class-instance/utils.ts`: pure date math for week generation (`generateInstancesForWeek`, `getWeekBounds`, `combineDateTime` — local-time semantics; `APP_TIMEZONE` env exists but is **never read in code**; server local TZ matters). All of this has bun tests (`*.test.ts`, integration tests against real PG) that can serve as the spec for the Rust port.

---

## 6. External services + environment

| Service | Usage |
|---|---|
| **Postgres 17** (docker-compose locally) | everything |
| **S3-compatible** (MinIO local / Cloudflare R2 prod) | one bucket (`S3_AVATAR_BUCKET_NAME`); presigned PUT (5 min), GET/PUT/DELETE/HEAD objects; `forcePathStyle: true` required for MinIO; public URL prefix `S3_AVATAR_PUBLIC_URL` |
| **sharp** | avatar pipeline: EXIF auto-rotate, cover-resize 256/64, webp q80 → Rust: `image` + `kamadak-exif` (or libvips binding) — verify EXIF rotation + webp quality parity |
| **Google OAuth** | via better-auth |
| **Resend** | env var exists, **never used** — password-reset email is a console.log stub |
| **Stripe** | columns + enums exist, **no integration code** — only `recordExternalPayment` manual entry |

**Required env vars** (names only): `DATABASE_URL`, `BETTER_AUTH_SECRET`, `NEXT_PUBLIC_BETTER_AUTH_URL` (client-bundled base URL; also read server-side, fallback `VITE_BETTER_AUTH_URL`), `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `RESEND_API_KEY` (unused), `APP_TIMEZONE` (unused in code), `S3_ENDPOINT`, `S3_ACCESS_KEY_ID`, `S3_SECRET_ACCESS_KEY`, `S3_AVATAR_BUCKET_NAME`, `S3_AVATAR_PUBLIC_URL`. Also referenced (not in .env.example): `BETTER_AUTH_URL`, `NEXT_PUBLIC_APP_URL` (fallbacks in `createUserAndInvite`).

---

## 7. Frontend coupling

- **Server-action imports are the coupling mechanism.** 16 client files import from `@/app/actions/*` (table in §1). To keep components untouched, replace each `app/actions/*.ts` file's body with a **client-side fetch shim of identical exported names/signatures** hitting the new Rust endpoints (e.g. `POST /api/actions/<module>/<fn>` with JSON body = the single `input` arg). This is the key trick that lets "frontend stays identical" hold at the component level.
  - Shim must reproduce: thrown errors for zod failures (components catch + toast), the `ActionResult` envelope for `user-bookings`, and **Date revival** — components receive `Date` objects today (e.g. `starts_at`, `payment_date`, `created_at` fields are formatted with `toLocaleString`-style calls). Either revive ISO strings to `Date` in the shim (recommended; a generic key-based reviver) or audit every consumer.
  - DECIMAL fields already arrive as strings today, so JSON changes nothing there.
- **Raw fetches:** only `/api/avatar` (profile page, admin customer page) + the presigned S3 PUT. Keep paths identical.
- **better-auth react client** (`src/lib/auth-client.ts`): `useSession`, `signIn.email/social`, `signUp.email`, `signOut` → wire-protocol parity required (§4).
- **next/* features in client code:** `next/link`, `next/navigation` (`useRouter`, `usePathname`, `useSearchParams`, `notFound`) everywhere — these are router-level; nextrs must provide its router equivalents or keep the Next-compatible client runtime. `notFound()` is called from **client** components in `[id]` pages (renders the 404 boundary client-side).
- **`next/image`: 0 uses. `next/font`: 0 uses.** Avatars are plain `<img>`; `UserAvatar` falls back to `facehash` (pure client). No image-optimizer or font-pipeline parity needed. `public/` assets (manifest.json, icons, hhh_logo.png) just need static serving.
- **No `server-only` imports, no shared isomorphic modules with server deps** — server code is cleanly isolated in `src/lib/core/*`, `src/lib/{auth,db,s3}.ts` (all become Rust); `src/lib/utils.ts`, `auth-client.ts`, components are client-clean.

---

## 8. Everything else a converter must know

- **Server actions: yes — the dominant pattern (12 modules / 68 fns).** nextrs almost certainly lacks the `'use server'` RPC; conversion strategy in §7.
- **`revalidatePath('/admin')`** in 4 class-instance mutations. `/admin` is already `force-dynamic` (re-fetches every request), and the schedule UI refetches client-side — these calls can be **dropped with no behavior change**.
- **ISR/`revalidate`: none. `export const runtime`/edge: none. generateStaticParams: none.** Only specials: `dynamic = 'force-dynamic'` on `/admin`.
- **Cron: none. WebSockets: none. SSE: none.**
- **PWA-ish metadata** (manifest, apple-web-app meta, safe-area CSS) — static head tags, easy parity.
- **Suspense/`useSearchParams`** pattern in login, customers, app layout — purely client React, unaffected.
- **Build pipeline:** `bun run build` = migrate-then-build; bun test suite (happy-dom preload) covers core modules incl. DB integration tests — keep them running against the same Postgres as a **behavioral spec** for the Rust port (port test cases, or run the TS tests against the Rust API where shapes allow).
- **`tsconfig` path alias** `@/*` → `src/*` plus `@/app/*`; `.cta.json`/README remnants mention TanStack (the app was scaffolded from a TanStack template then migrated) — ignore.
- **Known latent bugs to decide on (preserve vs fix):** ineffective `FOR UPDATE` in `deductCredit`; non-transactional `bookClass`/`createCustomerWithProduct`; actions trusting client-supplied `userId`; admin actions with no server-side authz.

---

## 9. Proposed conversion slices

Ordering principle: auth is the universal blocker; the action surface converts module-by-module behind a stable shim; the two RSC pages and the avatar pipeline are independent leaves.

| # | Slice | Contents | Depends on | Risk |
|---|---|---|---|---|
| 0 | **Skeleton** | Axum binary, config/env, PG pool, static assets + client bundle serving, 404/error pages, migration runner compatible with `schema_migrations` | – | Low |
| 1 | **Auth** | Decision (a) Rust reimpl vs (b) Bun sidecar proxied at `/api/auth/*`. Either way: Rust session extractor (cookie → `session` row → user+role) | 0 | **Highest** |
| 2 | **Route guards** | `proxy.ts` semantics as Axum middleware (redirect matrix + matcher exclusions, `?view=user`) | 1 | Low (logic is simple, in-process session check) |
| 3 | **Action shim + read-only modules** | Define `POST /api/actions/{module}/{fn}` convention + TS client shim w/ date revival & `ActionResult` envelope; port low-risk read/CRUD modules: products, class-templates, schedule-slots, dashboard, auth.searchUsers | 0 (1 for authz) | Medium (shim serialization parity is the risk) |
| 4 | **Scheduling engine** | class-instances (fillScheduleForWeek, week math from `utils.ts`), instances admin page actions | 3 | Medium (date/TZ semantics — server-local time, no APP_TIMEZONE) |
| 5 | **Booking engine** | bookings + user-bookings + user-credits + booking-limit logic; add real transactions | 3, 4 | **High** (most business logic; tests are the spec) |
| 6 | **CRM** | customers, customer-products, subscriptions, payments, createUserAndInvite (needs auth admin createUser — couples to slice 1 choice) | 1, 3 | Medium-high (createUserAndInvite + scrypt temp password; tri-state optionals; DECIMAL-as-string parity) |
| 7 | **Avatar pipeline** | `/api/avatar` GET/POST/DELETE in Rust: presign, image processing (EXIF rotate + webp), S3 writes, user.image update | 1 | Medium (sharp parity; verify iPhone EXIF photos) |
| 8 | **RSC pages → props.rs** | `/admin` seed (`getDashboardStats` + `getUpcomingInstances`) feeding `AdminDashboardClient`; `/admin/classes` seed (templates list); loading.tsx parity decision | 3, 4 | Low-medium (depends on nextrs seed ergonomics; `/admin/classes` renders RSC markup — may convert to a client page + seed to avoid RSC rendering in Rust) |
| 9 | **Cutover** | head/metadata parity, error boundary, env plumbing, deploy (replaces Vercel — single binary changes hosting story), drop `revalidatePath`, decide on unused stubs (Resend, validateSession, Stripe columns) | all | Low |

Parallelizable: 7 ∥ 3–6; 4 of the read-only modules in 3 are mutually independent. Hard sequence: 0 → 1 → (2, everything authz'd).

### Riskiest items (ranked)
1. **better-auth parity** — scrypt hash verification for existing users, session cookie format, `get-session` payload incl. `role`, Google OAuth flow. Mis-step logs everyone out or locks them out. Sidecar option de-risks this almost entirely.
2. **Server-action RPC replacement** — 68 functions; serialization drift (Dates, DECIMAL strings, `null` vs `undefined` tri-states, thrown-error vs `ActionResult` envelopes) will surface as subtle UI breakage across all 16 consuming files.
3. **Booking limit/credit engine** — per_week vs per_billing_period bucketing, credit FIFO, reactivation paths; port behind the existing bun test suite.
4. **Date/timezone semantics** — week generation and "next 7 days" windows use server-local time (`APP_TIMEZONE` env is dead); Rust default UTC would silently shift class times. Pin the TZ explicitly.
5. **Avatar image parity** — sharp's EXIF auto-rotate + cover crop + webp q80 reproduced in Rust; admin-on-behalf-of authz paths.

### Features used that nextrs likely lacks (parity decisions)
- **Server actions** (the whole API) — convert per §7. No way around it.
- **`middleware`/`proxy.ts`** — needs an Axum middleware equivalent (straightforward).
- **`loading.tsx` streaming fallbacks** for the 2 RSC pages; **client `notFound()`**; **`error.tsx` boundary** semantics.
- **Metadata/Viewport API** (manifest/apple tags) — static head emission.
- **NOT used (no work):** `next/image`, `next/font`, ISR/revalidate tags, edge runtime, route handlers beyond the 2 listed, rewrites/redirects/headers config, websockets, cron, draft mode, parallel/intercepting routes.
