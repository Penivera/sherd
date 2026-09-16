// PLACEHOLDER. Everything in this file is mock/demo data, not a real P2P
// network. It exists so the future network implementation has a single,
// obvious seam to replace (React UI -> this service -> [today: mock,
// future: real P2P client]) without the UI needing to change.

let connected = false;

export async function connectToMesh() {
  // Future: actually join the P2P mesh / register this client with it.
  connected = true;
  return { connected: true, nodeCount: 247 };
}

export function isConnectedToMesh() {
  return connected;
}

export async function disconnectFromMesh() {
  connected = false;
}
