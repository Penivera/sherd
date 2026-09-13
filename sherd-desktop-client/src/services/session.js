// AUTH SESSION lifecycle only. This is deliberately separate from:
//   - CLIENT PROCESS   (the Electron app being open at all)
//   - P2P/COMPUTE SESSION (future mesh participation, not implemented here)
// Closing/reopening the app does not by itself end the auth session; only
// an expired/removed token or an explicit logout does.

const STORAGE_KEY = "sherd.session.v1";

// Decode the JWT payload WITHOUT verifying the signature. This is only used
// client-side to read the "exp" claim for UI/session-lifecycle purposes.
// The backend is the sole source of truth for whether a token is actually
// valid; this never substitutes for server-side verification.
function decodeJwtPayload(token) {
  try {
    const [, payloadB64] = token.split(".");
    const json = atob(payloadB64.replace(/-/g, "+").replace(/_/g, "/"));
    return JSON.parse(json);
  } catch {
    return null;
  }
}

export function saveSession({ accessToken, user }) {
  const payload = decodeJwtPayload(accessToken);
  const expiresAt = payload?.exp ? payload.exp * 1000 : null; // ms epoch
  const session = { accessToken, user, expiresAt };
  localStorage.setItem(STORAGE_KEY, JSON.stringify(session));
  return session;
}

export function getSession() {
  const raw = localStorage.getItem(STORAGE_KEY);
  if (!raw) return null;

  let session;
  try {
    session = JSON.parse(raw);
  } catch {
    localStorage.removeItem(STORAGE_KEY);
    return null;
  }

  if (session.expiresAt && Date.now() >= session.expiresAt) {
    // Expired: remove it now rather than making every caller re-check.
    localStorage.removeItem(STORAGE_KEY);
    return null;
  }

  return session;
}

export function clearSession() {
  localStorage.removeItem(STORAGE_KEY);
}

export function getAccessToken() {
  return getSession()?.accessToken ?? null;
}

export function getCurrentSessionUser() {
  return getSession()?.user ?? null;
}
