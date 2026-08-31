import { createServer } from "node:http";

const HOST = "127.0.0.1";
const PORT = 59833;
let healthHits = 0;

const corsHeaders = {
  "access-control-allow-origin": "*",
  "cache-control": "no-store",
};

const server = createServer((request, response) => {
  const url = new URL(request.url ?? "/", `http://${HOST}:${PORT}`);
  for (const [name, value] of Object.entries(corsHeaders)) response.setHeader(name, value);
  if (url.pathname === "/ready") {
    response.end("ready");
    return;
  }
  if (url.pathname === "/health") {
    healthHits++;
    response.setHeader("content-type", "application/json");
    response.end(JSON.stringify({ status: "ok", api_version: 1 }));
    return;
  }
  if (url.pathname === "/__hits") {
    response.setHeader("content-type", "application/json");
    response.end(JSON.stringify({ healthHits }));
    return;
  }
  if (url.pathname === "/__reset" && request.method === "POST") {
    healthHits = 0;
    response.statusCode = 204;
    response.end();
    return;
  }
  response.statusCode = 404;
  response.end("Not Found");
});

server.listen(PORT, HOST);

console.log(`LNA health responder listening on http://${HOST}:${PORT}`);
