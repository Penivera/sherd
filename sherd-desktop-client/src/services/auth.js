// Thin client for the existing sherd-auth FastAPI service. Endpoint paths
// and request/response shapes here are copied directly from
// sherd-auth/app/routers/*.py and app/schemas.py — nothing here is invented.
//
//   POST /auth/register                {email, password}                 -> TokenResponse
//   POST /auth/login                   {email, password}                 -> TokenResponse
//   GET  /auth/me                      (Bearer)                          -> UserOut
//   GET  /auth/google/login            ?desktop_redirect_uri=...         -> browser redirect
//   GET  /auth/github/login            ?desktop_redirect_uri=...         -> browser redirect
//   POST /auth/token/exchange          {code}                            -> TokenResponse
//   POST /auth/solana/challenge        {wallet_address}                  -> {nonce, message, expires_at}
//   POST /auth/solana/verify           {wallet_address, nonce, signature}-> TokenResponse
//
// TokenResponse = { access_token, token_type, user: { id, email, providers } }

import { API_BASE_URL } from "./config";
import { saveSession, clearSession, getAccessToken } from "./session";

class AuthError extends Error {
  constructor(message, status) {
    super(message);
    this.name = "AuthError";
    this.status = status;
  }
}

async function postJson(path, body) {
  const res = await fetch(`${API_BASE_URL}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json().catch(() => null);
  if (!res.ok) {
    throw new AuthError(data?.detail || `Request to ${path} failed (${res.status})`, res.status);
  }
  return data;
}

function applyTokenResponse(tokenResponse) {
  return saveSession({
    accessToken: tokenResponse.access_token,
    user: tokenResponse.user,
  });
}

// --- Email/password ---

export async function registerWithEmail(email, password) {
  const data = await postJson("/auth/register", { email, password });
  return applyTokenResponse(data);
}

export async function loginWithEmail(email, password) {
  const data = await postJson("/auth/login", { email, password });
  return applyTokenResponse(data);
}

// --- OAuth (Google / GitHub) via desktop loopback redirect ---
// The backend's /auth/{provider}/login expects a desktop_redirect_uri it
// will bounce a one-time `code` back to once the browser-based OAuth dance
// finishes. electron/main.cjs owns spinning up that loopback listener and
// opening the system browser; this just talks to the resulting code.

export async function loginWithOAuthProvider(provider) {
  if (!window.sherd?.oauthLogin) {
    throw new AuthError(
      "OAuth login is only available inside the Sherd desktop app (no bridge found).",
      0
    );
  }
  const { code } = await window.sherd.oauthLogin(provider, API_BASE_URL);
  const data = await postJson("/auth/token/exchange", { code });
  return applyTokenResponse(data);
}

// --- Solana wallet challenge/response ---

export async function requestSolanaChallenge(walletAddress) {
  // -> { nonce, message, expires_at }
  return postJson("/auth/solana/challenge", { wallet_address: walletAddress });
}

export async function verifySolanaSignature({ walletAddress, nonce, signature }) {
  const data = await postJson("/auth/solana/verify", {
    wallet_address: walletAddress,
    nonce,
    signature,
  });
  return applyTokenResponse(data);
}

// --- Current user / logout ---

export async function fetchCurrentUser() {
  const token = getAccessToken();
  if (!token) throw new AuthError("Not authenticated", 401);

  const res = await fetch(`${API_BASE_URL}/auth/me`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  if (res.status === 401) {
    clearSession();
    throw new AuthError("Session expired", 401);
  }
  if (!res.ok) {
    throw new AuthError(`Failed to fetch current user (${res.status})`, res.status);
  }
  return res.json();
}

export function logout() {
  // Local only: clears the stored JWT. The backend issues stateless JWTs
  // (no server-side session/blacklist route exists), so there is nothing
  // additional to call server-side for a plain logout.
  clearSession();
}

export { AuthError };
