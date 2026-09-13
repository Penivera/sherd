// Wallet adapter layer for Solana challenge/response auth.
//
// IMPORTANT / KNOWN LIMITATION (see README "What is mocked"):
// Phantom and Solflare ship as browser extensions. Electron's renderer does
// not load Chrome extensions by default, so `window.solana` /
// `window.solflare` will normally NOT be injected inside this desktop app,
// even though the code path below checks for them (so a real adapter can
// be dropped in later, e.g. via a wallet-adapter package or a WalletConnect
// style flow, without changing anything above this file).
//
// For today's demo this falls back to an in-memory ed25519 keypair
// generated fresh in the renderer. It signs the EXACT challenge message the
// backend issues, and the backend verifies that signature for real with
// PyNaCl — so the challenge/response cryptography is genuinely functional
// end-to-end. The only thing that's "demo" is that the keypair isn't a real
// user-owned Phantom/Solflare wallet. The private key never leaves this
// module and is never sent anywhere; only the public address and
// signatures are.

import nacl from "tweetnacl";
import bs58 from "bs58";

function getInjectedAdapter() {
  if (typeof window === "undefined") return null;
  if (window.solana?.isPhantom) {
    return {
      kind: "phantom",
      async getAddress() {
        const resp = await window.solana.connect();
        return resp.publicKey.toString();
      },
      async signMessage(message) {
        const encoded = new TextEncoder().encode(message);
        const { signature } = await window.solana.signMessage(encoded, "utf8");
        return bs58.encode(signature);
      },
    };
  }
  return null;
}

// One ephemeral keypair per app session, created lazily.
let demoKeypair = null;

function getDemoAdapter() {
  return {
    kind: "demo",
    async getAddress() {
      if (!demoKeypair) demoKeypair = nacl.sign.keyPair();
      return bs58.encode(demoKeypair.publicKey);
    },
    async signMessage(message) {
      if (!demoKeypair) demoKeypair = nacl.sign.keyPair();
      const encoded = new TextEncoder().encode(message);
      const signature = nacl.sign.detached(encoded, demoKeypair.secretKey);
      return bs58.encode(signature);
    },
  };
}

// Returns the best available adapter: a real injected wallet if present,
// otherwise the demo keypair. Structured so a future PhantomAdapter /
// SolflareAdapter can be prioritized here without touching callers.
export function getWalletAdapter() {
  return getInjectedAdapter() || getDemoAdapter();
}
