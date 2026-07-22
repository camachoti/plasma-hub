import { platformFetch as tauriFetch } from '../../shared/platform/http';
import { runtimeCapabilities } from '../../shared/platform/runtime';

export interface ApiOptions {
  method?: 'GET' | 'POST';
  headers?: Record<string, string>;
  body?: any;
  rawText?: boolean;
}

export async function apiFetch(url: string, opts: ApiOptions = {}): Promise<any> {
  const fetchOpts: any = {
    method: opts.method ?? 'GET',
    headers: opts.headers,
  };
  if (opts.body !== undefined) {
    fetchOpts.body = typeof opts.body === 'string' ? opts.body : JSON.stringify(opts.body);
  }
  const res = await tauriFetch(url, fetchOpts);
  const text = await res.text();
  let data: any = text;
  if (!opts.rawText) {
    try { data = JSON.parse(text); } catch { /* keep as text */ }
  }
  const headers: Record<string, string> = {};
  res.headers.forEach((v: string, k: string) => { headers[k] = v; });
  return { status: res.status, data, text, headers };
}

export async function resolveRedirects(url: string): Promise<string> {
  try {
    if (runtimeCapabilities.isTauri) {
      const res = await apiFetch(url, { method: 'GET', headers: { 'User-Agent': 'Mozilla/5.0' }, rawText: true });
      return (res.headers['x-final-url'] ?? res.headers['location'] ?? url) as string;
    }
    const res = await fetch(url, { method: 'HEAD', mode: 'no-cors' });
    return res.url || url;
  } catch {
    return url;
  }
}
