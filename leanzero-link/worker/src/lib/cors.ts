export function corsHeadersFor(origin: string | null, allowedOrigins: string[]): Record<string, string> {
  if (allowedOrigins.includes("*")) {
    return { "Access-Control-Allow-Origin": "*" };
  }
  if (origin !== null && allowedOrigins.includes(origin)) {
    return { "Access-Control-Allow-Origin": origin, Vary: "Origin" };
  }
  return {};
}

export function preflightResponse(origin: string | null, allowedOrigins: string[]): Response {
  return new Response(null, {
    status: 204,
    headers: {
      ...corsHeadersFor(origin, allowedOrigins),
      "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
      "Access-Control-Allow-Headers": "Content-Type, Authorization",
      "Access-Control-Max-Age": "86400",
    },
  });
}

export function withCorsHeaders(response: Response, origin: string | null, allowedOrigins: string[]): Response {
  const entries = Object.entries(corsHeadersFor(origin, allowedOrigins));
  if (entries.length === 0) {
    return response;
  }
  const wrapped = new Response(response.body, response);
  for (const [name, value] of entries) {
    wrapped.headers.set(name, value);
  }
  return wrapped;
}
