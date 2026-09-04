const FEED_PATH = "/releases/latest.json";
const FEED_KEY = "latest.json";

function headers(): Headers {
  return new Headers({
    "Cache-Control": "public, max-age=300",
    "Content-Type": "application/json; charset=utf-8",
    "Cross-Origin-Embedder-Policy": "credentialless",
    "Cross-Origin-Opener-Policy": "same-origin",
    "X-Content-Type-Options": "nosniff",
  });
}

export async function handleRequest(request: Request, env: Env): Promise<Response> {
  const url = new URL(request.url);
  if (url.pathname !== FEED_PATH) {
    return new Response("Not Found", { status: 404 });
  }
  if (request.method !== "GET" && request.method !== "HEAD") {
    return new Response("Method Not Allowed", {
      status: 405,
      headers: { Allow: "GET, HEAD" },
    });
  }

  try {
    const body = await env.RELEASE_FEED.get(FEED_KEY, { type: "stream", cacheTtl: 60 });
    if (!body) {
      return new Response("Feed unavailable", {
        status: 503,
        headers: { "Cache-Control": "no-store" },
      });
    }
    return new Response(request.method === "HEAD" ? null : body, { headers: headers() });
  } catch (error) {
    console.error(
      JSON.stringify({
        message: "release feed read failed",
        error: error instanceof Error ? error.message : String(error),
      }),
    );
    return new Response("Feed unavailable", {
      status: 503,
      headers: { "Cache-Control": "no-store" },
    });
  }
}

export default {
  fetch: handleRequest,
} satisfies ExportedHandler<Env>;
