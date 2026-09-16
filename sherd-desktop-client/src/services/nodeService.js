// PLACEHOLDER. Mock node listing standing in for future real-time node
// discovery from the P2P/mesh network.

const MOCK_NODES = [
  { id: 1, price: "0.010" },
  { id: 2, price: "0.014" },
  { id: 3, price: "0.008" },
  { id: 4, price: "0.019" },
  { id: 5, price: "0.011" },
];

export async function listNodes() {
  // Future: fetch live node/pricing data from the mesh network.
  return MOCK_NODES;
}
