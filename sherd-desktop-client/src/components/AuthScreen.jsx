import React, { useState } from "react";
import { Mail, Github, Wallet, Loader2 } from "lucide-react";
import {
  loginWithEmail,
  registerWithEmail,
  loginWithOAuthProvider,
  requestSolanaChallenge,
  verifySolanaSignature,
  AuthError,
} from "../services/auth";
import { getWalletAdapter } from "../services/wallet";

const ORANGE = "#F16852";

function shortenAddress(address) {
  if (!address || address.length < 10) return address;
  return `${address.slice(0, 4)}…${address.slice(-4)}`;
}

export default function AuthScreen({ isDark, onAuthenticated }) {
  const [mode, setMode] = useState("login"); // login | register
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(null); // which action is in flight
  const [error, setError] = useState("");
  const [walletStatus, setWalletStatus] = useState(""); // visible proof of each step, for demo purposes

  const heading = isDark ? "text-white" : "text-neutral-900";
  const muted = isDark ? "text-neutral-400" : "text-neutral-500";
  const inputBg = isDark ? "bg-neutral-900 border-neutral-800 text-white" : "bg-white border-neutral-200 text-neutral-900";

  async function run(action, fn, extra) {
    setError("");
    setBusy(action);
    try {
      const session = await fn();
      onAuthenticated(session.user, extra);
    } catch (err) {
      setError(err instanceof AuthError ? err.message : "Something went wrong. Is the backend running?");
    } finally {
      setBusy(null);
      setWalletStatus("");
    }
  }

  async function handleEmailSubmit(e) {
    e.preventDefault();
    if (mode === "login") {
      run("email", () => loginWithEmail(email, password));
    } else {
      run("email", () => registerWithEmail(email, password));
    }
  }

  async function handleSolana() {
    setError("");
    setBusy("solana");
    let walletAddress = "";
    try {
      const adapter = getWalletAdapter();

      setWalletStatus("Getting wallet address…");
      walletAddress = await adapter.getAddress();

      setWalletStatus(`Requesting challenge for ${shortenAddress(walletAddress)}…`);
      const { nonce, message } = await requestSolanaChallenge(walletAddress);

      setWalletStatus(`Signing challenge (nonce ${nonce.slice(0, 8)}…)…`);
      const signature = await adapter.signMessage(message);

      setWalletStatus("Verifying signature with backend…");
      const session = await verifySolanaSignature({ walletAddress, nonce, signature });

      setWalletStatus("Verified ✓");
      onAuthenticated(session.user, { walletAddress, kind: adapter.kind });
    } catch (err) {
      setError(err instanceof AuthError ? err.message : "Something went wrong. Is the backend running?");
    } finally {
      setBusy(null);
      setTimeout(() => setWalletStatus(""), 1200);
    }
  }

  return (
    <div className="relative min-h-screen flex flex-col items-center justify-center px-6">
      <div className="relative z-10 w-full max-w-xs">
        <h1 className={`text-xl font-semibold mb-1 text-center ${heading}`}>
          {mode === "login" ? "Sign in to Sherd" : "Create your Sherd account"}
        </h1>
        <p className={`text-sm mb-6 text-center ${muted}`}>
          Authenticate to continue as a client.
        </p>

        <form onSubmit={handleEmailSubmit} className="space-y-2 mb-3">
          <input
            type="email"
            required
            placeholder="Email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            className={`w-full rounded-xl border-2 px-4 py-2.5 text-sm outline-none ${inputBg}`}
          />
          <input
            type="password"
            required
            minLength={8}
            placeholder="Password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className={`w-full rounded-xl border-2 px-4 py-2.5 text-sm outline-none ${inputBg}`}
          />
          <button
            type="submit"
            disabled={busy !== null}
            className="w-full text-sm font-medium rounded-xl py-2.5 text-white flex items-center justify-center gap-2 disabled:opacity-60"
            style={{ backgroundColor: ORANGE }}
          >
            {busy === "email" ? <Loader2 size={16} className="animate-spin" /> : <Mail size={16} />}
            {mode === "login" ? "Sign in with email" : "Register"}
          </button>
        </form>

        <button
          onClick={() => setMode(mode === "login" ? "register" : "login")}
          className={`w-full text-xs mb-5 ${muted}`}
        >
          {mode === "login" ? "No account? Register" : "Have an account? Sign in"}
        </button>

        <div className="flex items-center gap-3 mb-5">
          <div className={`h-px flex-1 ${isDark ? "bg-neutral-800" : "bg-neutral-200"}`} />
          <span className={`text-xs ${muted}`}>or</span>
          <div className={`h-px flex-1 ${isDark ? "bg-neutral-800" : "bg-neutral-200"}`} />
        </div>

        <div className="space-y-2">
          <button
            onClick={() => run("google", () => loginWithOAuthProvider("google"))}
            disabled={busy !== null}
            className={`w-full text-sm font-medium rounded-xl py-2.5 border-2 flex items-center justify-center gap-2 disabled:opacity-60 ${isDark ? "border-neutral-800 text-white" : "border-neutral-200 text-neutral-900"}`}
          >
            {busy === "google" ? <Loader2 size={16} className="animate-spin" /> : <span className="text-sm font-bold">G</span>}
            Continue with Google
          </button>
          <button
            onClick={() => run("github", () => loginWithOAuthProvider("github"))}
            disabled={busy !== null}
            className={`w-full text-sm font-medium rounded-xl py-2.5 border-2 flex items-center justify-center gap-2 disabled:opacity-60 ${isDark ? "border-neutral-800 text-white" : "border-neutral-200 text-neutral-900"}`}
          >
            {busy === "github" ? <Loader2 size={16} className="animate-spin" /> : <Github size={16} />}
            Continue with GitHub
          </button>
          <button
            onClick={handleSolana}
            disabled={busy !== null}
            className={`w-full text-sm font-medium rounded-xl py-2.5 border-2 flex items-center justify-center gap-2 disabled:opacity-60 ${isDark ? "border-neutral-800 text-white" : "border-neutral-200 text-neutral-900"}`}
          >
            {busy === "solana" ? <Loader2 size={16} className="animate-spin" /> : <Wallet size={16} />}
            Continue with Solana wallet
          </button>
        </div>

        {walletStatus && (
          <p className={`text-xs mt-4 text-center ${muted}`}>{walletStatus}</p>
        )}

        {error && (
          <p className="text-xs text-red-500 mt-4 text-center">{error}</p>
        )}
      </div>
    </div>
  );
}
