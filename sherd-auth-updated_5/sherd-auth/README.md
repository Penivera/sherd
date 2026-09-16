# Sherd Auth Service

Standalone FastAPI authentication/identity service for Sherd's desktop
client. One unified `User`, linkable to multiple `AuthIdentity` records
(email, Google, GitHub, Solana wallet).

## 1. What this service does

- Email + password registration/login
- Google and GitHub OAuth, designed for a **desktop client** (system
  browser + loopback handoff), not a browser-only web app
- Solana wallet auth via a real challenge/nonce flow: server issues a
  unique, expiring, single-use challenge; the wallet signs that exact
  message; the server verifies the Ed25519 signature and atomically
  consumes the challenge before issuing a session
- Resolves all of the above to **one Sherd `User`**, linking by email
  where applicable, so the rest of the system only ever deals with a
  single identity concept regardless of login method

## 2. What this service does NOT do

This is authentication/identity only. It does not implement, and has no
tables or endpoints for:

- P2P networking, node discovery, Kademlia/DHT, QUIC
- Job scheduling, WASM execution/sandboxing, compute execution
- Payments, payment channels, validator staking, reputation
- Any desktop UI — that's a separate client codebase

```
Desktop Client
      |
      | authentication
      v
Sherd Auth API  (this service)
      |
      +---- Email
      +---- Google      (desktop loopback handoff)
      +---- GitHub      (desktop loopback handoff)
      +---- Solana wallet (challenge/nonce)
      |
      v
Unified Sherd User Identity


Desktop Client
      |
      v
Sherd Network / P2P Layer   (owned by another team, not in this repo)
      |
      +---- Provider Nodes
      +---- Discovery
      +---- Job Execution
```

The desktop client authenticates against this service, gets a JWT +
unified user, and *separately* uses that identity to talk to the P2P/
execution layer. This service has no knowledge of nodes, jobs, or pricing.

## 3. Stack

FastAPI · SQLAlchemy 2.0 · Alembic · PostgreSQL · Pydantic v2 / pydantic-settings · uv

## 4. Project structure

```
app/
  main.py              FastAPI app, middleware, router registration
  config.py            pydantic-settings (.env)
  database.py          engine/session, Base
  db_types.py          cross-dialect GUID (Postgres UUID / SQLite CHAR(36) for tests)
  dependencies.py       get_current_user (Bearer JWT)
  security.py          password hashing, JWT, secure tokens, loopback validation
  models.py            User, AuthIdentity, SolanaChallenge, OAuthExchangeCode
  schemas.py           Pydantic request/response models
  serializers.py       User -> UserOut
  oauth_clients.py     authlib OAuth client registration (Google, GitHub)
  oauth_common.py      find-or-create/link user; exchange-code mint/consume
  routers/
    auth_email.py      register / login / me
    auth_google.py     login (redirect) / callback (loopback handoff or direct token)
    auth_github.py     login (redirect) / callback (loopback handoff or direct token)
    auth_solana.py     challenge / verify
    auth_token.py       token/exchange (redeems a loopback one-time code)
migrations/
  versions/0001_initial_auth_schema.py                       users, auth_identities
  versions/0002_solana_challenges_and_exchange_codes.py       solana_challenges, oauth_exchange_codes
tests/
```

## 5. Database schema

```
users
  id (uuid, pk)
  email (varchar, unique, nullable)
  password_hash (varchar, nullable)
  created_at, updated_at

auth_identities
  id (uuid, pk)
  user_id (uuid, fk -> users.id, on delete cascade)
  provider (enum: email | google | github | solana)
  provider_account_id (varchar)   -- email address / OAuth sub / wallet pubkey
  provider_metadata (varchar, nullable)
  created_at
  UNIQUE (provider, provider_account_id)
  INDEX  (user_id)

solana_challenges
  id (uuid, pk)
  wallet_address (varchar)
  nonce (varchar, unique)          -- opaque handle the client sends back
  message (varchar)                -- exact text the wallet must sign; server-generated
  created_at, expires_at, consumed_at (nullable)
  INDEX (wallet_address), INDEX/UNIQUE (nonce)

oauth_exchange_codes
  id (uuid, pk)
  code_hash (varchar, unique)      -- sha256 of the one-time code; raw code never stored
  user_id (uuid, fk -> users.id, on delete cascade)
  created_at, expires_at, consumed_at (nullable)
  INDEX/UNIQUE (code_hash)
```

`solana_challenges` and `oauth_exchange_codes` are minimal, purpose-built
placeholders for auth mechanics only — not general-purpose queues or
session stores.

## 6. Environment variables

See `.env.example`. Required:

- `DATABASE_URL` — e.g. `postgresql+psycopg://user:pass@localhost:5432/sherd_auth`
- `JWT_SECRET_KEY` — long random string (also used to sign the session
  cookie that carries OAuth `state` + the desktop loopback redirect
  through the browser round trip)
- `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET` / `GOOGLE_REDIRECT_URI`
- `GITHUB_CLIENT_ID` / `GITHUB_CLIENT_SECRET` / `GITHUB_REDIRECT_URI`

Optional (same defaults, override if needed):

- `JWT_ALGORITHM` (default `HS256`), `JWT_EXPIRE_MINUTES` (default `60`)
- `FRONTEND_URL`
- `SOLANA_CHALLENGE_TTL_SECONDS` (default `300`) — how long a Solana
  challenge stays valid before it must be rejected
- `OAUTH_EXCHANGE_CODE_TTL_SECONDS` (default `60`) — how long the
  desktop app has to redeem the loopback one-time code

**Note:** the `GOOGLE_REDIRECT_URI` / `GITHUB_REDIRECT_URI` env vars are
this *backend's* registered callback with Google/GitHub (a fixed
`http://localhost:8000/...` URL you register in each provider's console).
They are **not** the desktop app's loopback address — that's passed
per-request as `desktop_redirect_uri` (see §10).

## 7. PostgreSQL setup

```bash
createdb sherd_auth
# or: docker run -e POSTGRES_PASSWORD=sherd -e POSTGRES_USER=sherd \
#       -e POSTGRES_DB=sherd_auth -p 5432:5432 -d postgres:16
```

Set `DATABASE_URL` in `.env` to point at it, matching the credentials above.

## 8. Running locally

```bash
uv sync                       # installs from pyproject.toml (uv.lock included)
cp .env.example .env          # fill in real DB/OAuth values
uv run alembic upgrade head
uv run uvicorn app.main:app --reload
```

Interactive API docs at `http://localhost:8000/docs`.

## 9. Migrations

```bash
uv run alembic upgrade head                              # apply all
uv run alembic downgrade -1                               # roll back one
uv run alembic revision --autogenerate -m "message"        # generate a new one later
```

Both `0001_initial_auth_schema.py` and `0002_solana_challenges_and_exchange_codes.py`
were written by hand against the models (matching them exactly) rather
than autogenerated, since no live Postgres instance was reachable from
this environment to run `alembic revision --autogenerate` against.
**Run `alembic upgrade head` against a real Postgres and review the
result before trusting it in production**, and autogenerate a follow-up
migration if anything's off.

## 10. Running tests

```bash
uv run pytest -v
```

23 tests, all passing, against an in-memory SQLite DB (via the
cross-dialect `GUID` type in `app/db_types.py`; native `UUID` on
Postgres) with all external Google/GitHub network calls mocked — no real
OAuth credentials or a real Solana wallet needed to run the suite.

Covers: email registration, duplicate email, wrong password, `/auth/me`
with/without a token; Google/GitHub callback success, OAuth failure,
account linking by email, no-duplicate-identity on repeat login, the
desktop loopback handoff end-to-end (login → callback → code →
`/auth/token/exchange`, including single-use and non-loopback-redirect
rejection); and Solana valid signature, invalid signature, malformed
wallet address, reused challenge, expired challenge, wrong-wallet-for-
challenge, unknown nonce, and same-wallet-reuses-same-user.

## 11. Email authentication flow

```
POST /auth/register {email, password}  -> 201 + {access_token, user}
POST /auth/login    {email, password}  -> 200 + {access_token, user}
GET  /auth/me        (Authorization: Bearer <token>) -> UserOut
```

Passwords are hashed with argon2 (via passlib). Sessions are stateless
JWTs (`sub` = user id, `exp` = now + `JWT_EXPIRE_MINUTES`).

## 12. Google / GitHub desktop OAuth flow (loopback handoff)

This backend is the OAuth **confidential client** (it holds the
`client_secret` with Google/GitHub) — the desktop app never talks to
Google/GitHub directly. The desktop app only needs to: open a browser,
listen on a local port, and call one exchange endpoint.

```
1. Desktop app starts a local HTTP listener on an ephemeral loopback
   port, e.g. http://127.0.0.1:53214/callback

2. Desktop app opens the system browser to:
     GET /auth/{google|github}/login?desktop_redirect_uri=http://127.0.0.1:53214/callback
   The backend validates desktop_redirect_uri is a loopback address
   (127.0.0.1 / localhost / ::1 — anything else is rejected with 400),
   stashes it in a signed session cookie, and redirects to the provider.

3. User approves access in the browser. Google/GitHub redirects back to
   THIS backend's fixed, provider-registered callback
   (GOOGLE_REDIRECT_URI / GITHUB_REDIRECT_URI).

4. Backend's /auth/{google|github}/callback exchanges the code with the
   provider, resolves/creates the unified Sherd User, mints a short-lived
   (60s default) single-use exchange code, and redirects the browser to:
     http://127.0.0.1:53214/callback?code=<one-time-code>
   (If no desktop_redirect_uri was set in step 2 — e.g. someone hits the
   login URL directly in a plain browser to test — the callback instead
   returns {access_token, user} directly as JSON.)

5. Desktop app's local listener receives the code from that redirect and
   calls:
     POST /auth/token/exchange {"code": "<one-time-code>"}
     -> 200 {access_token, user}     (same shape as every other login)
   The code is single-use; redeeming it twice returns 401.
```

The JWT itself is never put in a URL/redirect — only the one-time code
is, which is useless without hitting `/auth/token/exchange` before it
expires or gets used once.

**PKCE:** the browser-facing OAuth 2.0 exchange between this backend and
Google/GitHub is handled by `authlib`, which supports PKCE where the
provider does (Google); GitHub's OAuth app flow doesn't support PKCE.
Since this backend is a confidential client (not a public client
embedded in the desktop app), the primary protections here are the
loopback-only redirect allowlist and the short-lived single-use exchange
code, not PKCE on the desktop side.

## 13. Solana wallet authentication flow

```
1. POST /auth/solana/challenge {"wallet_address": "<base58 pubkey>"}
   -> 200 {"nonce": "...", "message": "<exact text to sign>", "expires_at": "..."}
   Server generates a random nonce, builds the message server-side
   (embeds the wallet address, nonce, issued-at and expires-at), and
   stores it — the client never gets to choose or influence this text.

2. Desktop client asks the wallet (e.g. Phantom) to sign `message` byte-
   for-byte and gets back a base58 signature.

3. POST /auth/solana/verify {"wallet_address", "nonce", "signature"}
   -> 200 {access_token, user}   on success
   -> 401 on: unknown nonce, expired challenge, already-used challenge,
      wallet_address not matching the one the challenge was issued for,
      or a cryptographically invalid signature.

   Verification always re-checks the signature against the SERVER'S
   stored message for that nonce, never a client-supplied message
   string, and the challenge is atomically marked consumed via an
   UPDATE ... WHERE consumed_at IS NULL AND expires_at > now() — so two
   concurrent verify attempts against the same nonce can't both succeed.
```

No private keys or seed phrases are ever sent to or requested by this
backend — only a public key and a signature.

## 14. Full endpoint list

| Method & path | Body / params | Notes |
|---|---|---|
| `POST /auth/register` | `{email, password}` | 201 + token. 409 if email taken. |
| `POST /auth/login` | `{email, password}` | 200 + token. 401 on bad credentials. |
| `GET /auth/me` | Bearer token | Returns `UserOut`. 401 if missing/invalid. |
| `GET /auth/google/login` | `?desktop_redirect_uri=` (optional) | Redirects to Google. 400 if `desktop_redirect_uri` isn't loopback. |
| `GET /auth/google/callback` | query params from Google | Loopback redirect with `?code=` if a desktop redirect was set; else 200 + token directly. 400 on OAuth failure. |
| `GET /auth/github/login` | `?desktop_redirect_uri=` (optional) | Same pattern as Google. |
| `GET /auth/github/callback` | query params from GitHub | Same pattern as Google. |
| `POST /auth/solana/challenge` | `{wallet_address}` | 200 + `{nonce, message, expires_at}`. 400 on malformed address. |
| `POST /auth/solana/verify` | `{wallet_address, nonce, signature}` | 200 + token. 401 on any challenge/signature problem. |
| `POST /auth/token/exchange` | `{code}` | 200 + token. 401 if invalid/expired/already used. |
| `GET /health` | — | Liveness check. |

Every endpoint that issues a session returns the same shape:

```json
{
  "access_token": "<jwt>",
  "token_type": "bearer",
  "user": { "id": "<uuid>", "email": "a@b.com", "providers": ["email", "google"] }
}
```

Send it back as `Authorization: Bearer <access_token>`.

There is no `/auth/logout` — sessions are stateless JWTs with an expiry
(`JWT_EXPIRE_MINUTES`); logout is a client-side token discard. Server-side
revocation (a token blocklist, or moving to server-tracked sessions) is a
deliberate out-of-scope addition — flag it if the desktop client needs it.

## 15. Assumptions made

- No email verification flow. Structured so `AuthIdentity(provider=email)`
  could gain a `verified_at` column later without a redesign.
- Account linking by email: a Google/GitHub login whose email matches an
  existing user is attached to that user rather than creating a
  duplicate. Solana has no email, so it can only create a new user or
  match its own existing wallet identity — never auto-links to an
  email-based account.
- JWT bearer tokens in the response body (not cookies), since no
  desktop-client session-storage preference was specified.
- Desktop loopback redirect is restricted to `127.0.0.1` / `localhost` /
  `::1`; the exact port is left to the desktop app to choose per-launch.
- Solana challenge TTL defaults to 5 minutes, exchange-code TTL to 60
  seconds — both configurable, chosen as reasonable defaults for "sign
  this now" flows rather than anything long-lived.

## 16. What cannot be fully verified without real credentials

- **Google/GitHub OAuth end-to-end against the real providers** — tests
  mock `authorize_access_token`/`get` at the `authlib` client boundary,
  which validates our callback logic, session handling, and the loopback
  handoff, but does not exercise Google's/GitHub's real authorization
  servers, real PKCE negotiation, or real consent screens. `GITHUB_CLIENT_ID`/
  `GITHUB_CLIENT_SECRET` are also blank in `.env` — you'll need to
  register an OAuth app with GitHub and fill these in to test manually.
- **A real Solana wallet (e.g. Phantom) signing the challenge message**
  — tests use an in-memory `nacl` keypair to sign, which validates the
  Ed25519 verification and the challenge lifecycle correctly, but a
  manual pass with an actual wallet extension is worth doing before
  shipping (a scratch `test.html` using Phantom's `window.phantom.solana`
  is included in the repo root for this).
- **The Alembic migrations against a real Postgres** — see §9; validated
  for syntax/revision-chain correctness here, not run against Postgres
  in this environment.
- **Concurrency behavior under real load** — the atomic-consume UPDATEs
  for both Solana challenges and OAuth exchange codes are correct
  per-statement, but haven't been load-tested for actual concurrent
  request handling.
