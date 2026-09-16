// Thin client for sherd-vm IPC + HTTP bridge.
// Mirrors meshService.js / nodeService.js pattern: one seam for the UI to
// call, with mock fallback when the daemon is not running.
//
// Two transports:
//  1) IPC via Electron preload (window.sherd.vm*) when running inside Electron
//  2) Direct HTTP to sherd-vm daemon (http://localhost:8765) when available
//  3) Mock fallback for demo without live Solari key

import { getAccessToken } from "./session.js";
import { API_BASE_URL } from "./config.js";

const VM_HTTP_BASE = import.meta.env.VITE_VM_BASE_URL || "http://localhost:8765";
const VM_IPC_SOCKET = "sherd-vm.sock";

// --- Helpers ---

function authHeaders() {
  const token = getAccessToken();
  return token ? { Authorization: `Bearer ${token}` } : {};
}

async function vmFetch(path, opts = {}) {
  const url = `${VM_HTTP_BASE}${path}`;
  const res = await fetch(url, {
    ...opts,
    headers: {
      "Content-Type": "application/json",
      ...authHeaders(),
      ...(opts.headers || {}),
    },
  });
  const data = await res.json().catch(() => null);
  if (!res.ok) {
    throw new Error(data?.message || data?.error || `VM request failed (${res.status})`);
  }
  return data;
}

// --- IPC via Electron preload (if available) ---

function hasIpc() {
  return typeof window !== "undefined" && window.sherd?.vmRequest;
}

async function viaIpc(request) {
  if (!hasIpc()) throw new Error("no IPC bridge");
  return window.sherd.vmRequest(request);
}

// --- Public API ---

/**
 * Provision a new VM.
 * @param {object} opts - { os: 'linux'|'windows', template, resolution, cpu, memMb, timeoutMs, lifecycle }
 * @returns {Promise<{session_id, stream_url, status}>}
 */
export async function createVm(opts = {}) {
  const payload = {
    os: opts.os || "linux",
    template: opts.template || "default",
    resolution: opts.resolution || "1280x720",
    cpu: opts.cpu,
    mem_mb: opts.memMb,
    timeout_ms: opts.timeoutMs,
    lifecycle: opts.lifecycle || "pause",
    from_snapshot: opts.fromSnapshot,
  };

  // Try IPC first, then HTTP, then mock
  if (hasIpc()) {
    try {
      const res = await viaIpc({ type: "Create", payload });
      if (res.type === "Created" || res.type === "Session") return res.payload;
      throw new Error(res.payload?.message || "create failed");
    } catch (e) {
      console.warn("[vmService] IPC create failed, trying HTTP:", e.message);
    }
  }

  try {
    return await vmFetch("/vm/create", { method: "POST", body: JSON.stringify(payload) });
  } catch (e) {
    console.warn("[vmService] HTTP create failed, using mock:", e.message);
    return mockCreateVm(payload);
  }
}

export async function getVmStatus(sessionId) {
  if (hasIpc()) {
    try {
      const res = await viaIpc({ type: "Status", payload: { session_id: sessionId } });
      if (res.type === "Status") return res.payload;
    } catch {}
  }
  try {
    return await vmFetch(`/vm/${encodeURIComponent(sessionId)}/status`);
  } catch {
    return mockStatus(sessionId);
  }
}

export async function destroyVm(sessionId) {
  if (hasIpc()) {
    try {
      await viaIpc({ type: "Destroy", payload: { session_id: sessionId } });
      return;
    } catch {}
  }
  try {
    await vmFetch(`/vm/${encodeURIComponent(sessionId)}`, { method: "DELETE" });
  } catch (e) {
    console.warn("[vmService] destroy failed:", e.message);
  }
}

export async function getStreamUrl(sessionId) {
  if (hasIpc()) {
    try {
      const res = await viaIpc({ type: "StreamUrl", payload: { session_id: sessionId } });
      if (res.type === "StreamUrl") return res.payload.url;
    } catch {}
  }
  try {
    const data = await vmFetch(`/vm/${encodeURIComponent(sessionId)}/stream`);
    return data.url || data.stream_url;
  } catch {
    return mockStreamUrl(sessionId);
  }
}

export async function uploadFile(sessionId, localFile, remotePath) {
  // localFile is a File object from <input type="file">
  const bytes = await localFile.arrayBuffer();
  const b64 = btoa(String.fromCharCode(...new Uint8Array(bytes)));

  if (hasIpc()) {
    try {
      await viaIpc({
        type: "Upload",
        payload: { session_id: sessionId, path: remotePath, content_base64: b64 },
      });
      return;
    } catch {}
  }

  await vmFetch(`/vm/${encodeURIComponent(sessionId)}/upload`, {
    method: "POST",
    body: JSON.stringify({ path: remotePath, content_base64: b64 }),
  });
}

export async function sendInput(sessionId, event) {
  // event: { type: 'mouse_move'|'mouse_click'|'key_type'|'key_press', x, y, button, text, keys, humanize }
  if (hasIpc()) {
    try {
      await viaIpc({ type: "Input", payload: { session_id: sessionId, event } });
      return;
    } catch {}
  }
  await vmFetch(`/vm/${encodeURIComponent(sessionId)}/input`, {
    method: "POST",
    body: JSON.stringify({ event }),
  });
}

export async function captureScreenshot(sessionId, format = "png") {
  if (hasIpc()) {
    try {
      const res = await viaIpc({
        type: "Screenshot",
        payload: { session_id: sessionId, format, quality: null },
      });
      if (res.type === "Screenshot") return `data:image/${format};base64,${res.payload.data_base64}`;
    } catch {}
  }
  try {
    const data = await vmFetch(`/vm/${encodeURIComponent(sessionId)}/screenshot?format=${format}`);
    return `data:image/${format};base64,${data.data_base64}`;
  } catch {
    return null;
  }
}

export async function execCommand(sessionId, cmd, args = []) {
  if (hasIpc()) {
    try {
      const res = await viaIpc({ type: "Exec", payload: { session_id: sessionId, cmd, args } });
      if (res.type === "Exec") return res.payload;
    } catch {}
  }
  return vmFetch(`/vm/${encodeURIComponent(sessionId)}/exec`, {
    method: "POST",
    body: JSON.stringify({ cmd, args }),
  });
}

export async function listVms() {
  if (hasIpc()) {
    try {
      const res = await viaIpc({ type: "List", payload: null });
      if (res.type === "List") return res.payload;
    } catch {}
  }
  try {
    return await vmFetch("/vm/list");
  } catch {
    return [];
  }
}

// --- Mock fallbacks (demo without live Solari key) ---

function mockCreateVm(opts) {
  const id = `mock_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`;
  console.info("[vmService] mock VM created:", id, opts);
  return {
    session_id: id,
    stream_url: null, // no real stream in mock
    status: "mock",
    template: opts.template,
    os: opts.os,
    _mock: true,
  };
}

function mockStatus(sessionId) {
  return {
    session_id: sessionId,
    state: "up",
    detail: "mock — no live Solari VM (set SOLARI_API_KEY and run sherd-vm daemon)",
    stream_url: null,
    os: "linux",
    _mock: true,
  };
}

function mockStreamUrl(sessionId) {
  // Return null to signal mock; UI should show placeholder
  console.info("[vmService] mock stream_url for", sessionId);
  return null;
}
