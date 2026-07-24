const AZTEC_NODE_URL = process.env.AZTEC_NODE_URL || "http://localhost:8080";
const isLocalNetwork = AZTEC_NODE_URL.includes("localhost") || AZTEC_NODE_URL.includes("127.0.0.1");

// 5.0.0 nodes reject a plain GET /status with 405 — probe via the node_getNodeInfo JSON-RPC.
async function isServiceHealthy(url: string): Promise<boolean> {
  try {
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", method: "node_getNodeInfo", params: [], id: 1 }),
      signal: AbortSignal.timeout(5000),
    });
    return res.ok;
  } catch {
    return false;
  }
}

export async function assertServicesAvailable(): Promise<void> {
  const aztec = await isServiceHealthy(AZTEC_NODE_URL);
  if (!aztec) {
    const hint = isLocalNetwork
      ? "Start Aztec local network before running fullstack e2e tests.\n  aztec start --local-network"
      : `Aztec node at ${AZTEC_NODE_URL} is unreachable — it may be down.`;
    throw new Error(`Aztec node not available at ${AZTEC_NODE_URL}. ${hint}`);
  }
}
