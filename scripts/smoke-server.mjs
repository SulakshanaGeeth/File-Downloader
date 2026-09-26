import { createServer } from "node:http";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Run: node scripts/smoke-server.mjs [port]
// Only loopback clients can connect. All successful downloads contain plain text.
// Add /sample.txt twice to verify automatic filename collision handling.
// Add /slow.txt four times, then close Flow, to verify active/queued recovery.
export const endpoints = Object.freeze({
  "/sample.txt": { chunks: 16, knownSize: true, description: "128 KiB, known size, about 8 seconds" },
  "/stream.txt": { chunks: 16, knownSize: false, description: "128 KiB, unknown size, about 8 seconds" },
  "/slow.txt": { chunks: 120, knownSize: true, description: "960 KiB, known size, about 60 seconds" },
  "/missing.txt": { description: "HTTP 404 failure" },
});

const chunk = Buffer.alloc(8192, "Flow desktop smoke test: deterministic plain text.\n");
const intervalMs = 500;

export function createSmokeServer() {
  return createServer((request, response) => {
    response.setHeader("Content-Type", "text/plain; charset=utf-8");
    response.setHeader("Cache-Control", "no-store");
    if (request.method !== "GET" && request.method !== "HEAD") {
      response.writeHead(405, { Allow: "GET, HEAD" });
      response.end("Use GET or HEAD.\n");
      return;
    }

    const pathname = new URL(request.url ?? "/", "http://127.0.0.1").pathname;
    const endpoint = Object.hasOwn(endpoints, pathname) ? endpoints[pathname] : undefined;
    if (!endpoint?.chunks) {
      response.writeHead(404);
      response.end("This file is intentionally unavailable.\n");
      return;
    }

    if (endpoint.knownSize) {
      response.setHeader("Content-Length", chunk.length * endpoint.chunks);
    }
    response.writeHead(200);
    if (request.method === "HEAD") {
      response.end();
      return;
    }
    response.flushHeaders();

    let sent = 0;
    let timer;
    const send = () => {
      if (response.destroyed) return;
      sent += 1;
      const ready = response.write(chunk);
      if (sent === endpoint.chunks) {
        response.end();
      } else if (ready) {
        timer = setTimeout(send, intervalMs);
      } else {
        response.once("drain", () => { timer = setTimeout(send, intervalMs); });
      }
    };
    response.on("close", () => clearTimeout(timer));
    response.on("error", () => clearTimeout(timer));
    timer = setTimeout(send, intervalMs);
  });
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const port = Number(process.argv[2] ?? 47831);
  if (!Number.isInteger(port) || port < 0 || port > 65535) {
    console.error("Usage: node scripts/smoke-server.mjs [port 0-65535]");
    process.exitCode = 1;
  } else {
    const server = createSmokeServer();
    server.on("error", (error) => {
      console.error(`Could not start smoke server: ${error.message}`);
      process.exitCode = 1;
    });
    server.listen(port, "127.0.0.1", () => {
      const address = server.address();
      console.log("Flow local smoke server. Stop with Ctrl+C.");
      for (const [path, endpoint] of Object.entries(endpoints)) {
        console.log(`http://127.0.0.1:${address.port}${path} - ${endpoint.description}`);
      }
    });
    const stop = () => {
      server.close();
      server.closeAllConnections();
    };
    process.once("SIGINT", stop);
    process.once("SIGTERM", stop);
  }
}
