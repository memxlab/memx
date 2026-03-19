function buildUrl(baseUrl, path, query = {}) {
  const url = new URL(path, `${baseUrl}/`);
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null || value === "") continue;
    url.searchParams.set(key, String(value));
  }
  return url;
}

export async function request(cfg, method, path, options = {}) {
  const {
    query,
    body,
    timeoutMs = 8000,
  } = options;

  const url = buildUrl(cfg.baseUrl, path, query);
  const headers = {
    accept: "application/json",
  };

  const init = {
    method,
    headers,
    signal: AbortSignal.timeout(timeoutMs),
  };

  if (body !== undefined) {
    headers["content-type"] = "application/json";
    init.body = JSON.stringify(body);
  }

  const response = await fetch(url, init);
  const text = await response.text();

  let data = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = text;
    }
  }

  if (!response.ok) {
    const detail = typeof data === "string" ? data : JSON.stringify(data);
    throw new Error(
      `memx ${method} ${url.pathname} failed: HTTP ${response.status}${detail ? ` ${detail}` : ""}`,
    );
  }

  return data;
}
