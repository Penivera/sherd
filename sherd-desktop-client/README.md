# Sherd Desktop Client (demo build)

Electron shell around the existing Sherd React UI, wired to the `sherd-auth`
FastAPI backend. Built for a same-day demo — see "What's mocked" below for
exactly where the edges are.

## 1. Requirements

- Node.js 18+ and npm
- Python environment for `sherd-auth` (already set up on your end) with a
  reachable Postgres `DATABASE_URL`
- One small backend change (see step 4) — a CORS middleware addition, not a
  rewrite

## 2. Install

```bash
npm install
```

## 3. Environment variables

```bash
cp .env.example .env
```

`.env`:

```
VITE_API_BASE_URL=http://localhost:8000
```

## 4. Start the backend

The desktop client's renderer runs on `http://localhost:5173` (a different
origin than the API on `:8000`), and `sherd-auth/app/main.py` did not have
CORS configured, so browser `fetch()` calls from the client would be
blocked. Add this once to `sherd-auth/app/main.py` (also included as a
ready-to-copy patch alongside this README):

```python
from fastapi.middleware.cors import CORSMiddleware
# ...
app.add_middleware(
    CORSMiddleware,
    allow_origins=["http://localhost:5173"],
    allow_methods=["*"],
    allow_headers=["*"],
)
```

Then, from `sherd-auth/`:

```bash
uvicorn app.main:app --reload
```

Make sure `sherd-auth/.env` has real values for `DATABASE_URL`,
`JWT_SECRET_KEY`, and (if you want to demo Google/GitHub) the OAuth client
ID/secret/redirect-URI settings — those are unchanged from your existing
backend setup.

## 5. Run the desktop app

```bash
npm run dev
```

This starts the Vite dev server and launches the Electron window once it's
ready. Closing the window quits the app (except on macOS).

Production-style build (bundles the React app; still launches via Electron,
not packaged as an installer):

```bash
npm run build
npm start
```

Full installer via electron-builder (`npm run dist`) is wired up in
`package.json` but untested today — treat it as a starting point, not a
demo-day dependency.

## 6. How authentication works

- **Email/password**: the form posts straight to `POST /auth/login` or
  `POST /auth/register`.
- **Google / GitHub**: the app asks Electron's main process to (1) open a
  loopback HTTP listener on a random `127.0.0.1` port, (2) open your system
  browser at `GET /auth/{provider}/login?desktop_redirect_uri=http://127.0.0.1:<port>/callback`.
  After you finish the provider's login in the browser, `sherd-auth`
  redirects the browser back to that loopback URL with a one-time `?code=`.
  The renderer exchanges it via `POST /auth/token/exchange` for the real JWT.
- **Solana wallet**: requests a challenge via `POST /auth/solana/challenge`,
  signs the exact returned message, and verifies via
  `POST /auth/solana/verify`. See "What's mocked" — today this signs with an
  in-app demo keypair, not Phantom.
- The JWT is stored in `localStorage` (renderer-side, demo-appropriate) with
  its expiry read from the token's `exp` claim. `GET /auth/me` returning 401
  or a locally-detected expiry both drop the user back to the auth screen.
  Logging out only clears the local token — there's no server-side session
  to invalidate for a stateless JWT.

## 7. How the client talks to the API

All calls go through `src/services/auth.js`, using `VITE_API_BASE_URL`.
Nothing is hardcoded to a specific host, and no endpoint here was invented —
each one is copied from `sherd-auth/app/routers/*.py`.

## 8. What is genuinely functional

- Electron desktop window loading the real React UI (splash → auth → power
  on → mesh)
- Email login/register against your actual backend, including real
  password hashing/verification server-side
- Google/GitHub OAuth desktop loopback flow, using your backend's actual
  exchange-code design
- Solana challenge/response: a real ed25519 keypair signs the exact
  server-issued challenge, and your backend verifies that signature for
  real with PyNaCl. Cryptographically this is the real flow — only the
  wallet identity is a demo keypair, not a Phantom-owned one (see below)
- JWT storage, expiry detection, attaching `Authorization: Bearer`, and
  logout

## 9. What remains mocked

- **Phantom/Solflare wallets**: browser-extension wallets don't inject into
  Electron's renderer by default (Electron doesn't load Chrome extensions).
  `src/services/wallet.js` checks for `window.solana` first and falls back
  to an in-memory demo keypair when it's absent — which, today, it always
  will be. The adapter interface is there so swapping in a real
  `@solana/wallet-adapter` integration later doesn't touch any calling code.
- **Mesh/node/task data**: "247 nodes online", the node price list, and task
  activity are mock data, unchanged from the original UI, now routed through
  `src/services/meshService.js`, `nodeService.js`, and `taskService.js` so a
  future P2P implementation has one clear place to plug into instead of
  scattered UI state.
- **Everything P2P**: no networking, DHT, job execution, or payments — out
  of scope today by design.

## 10. Files to hand to the rest of the team

- This whole `sherd-desktop-client/` project
- The one-line CORS patch to `sherd-auth/app/main.py` (needs to land in the
  backend repo)
- This README
