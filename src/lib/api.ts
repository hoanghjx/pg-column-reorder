let token = ''

interface ApiOptions extends RequestInit {
  body?: string
}

export async function api<T>(path: string, options: ApiOptions = {}): Promise<T> {
  const response = await fetch(path, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...options.headers,
    },
  })
  const body = await response.json()
  if (!response.ok) throw new Error(body.error || 'Request failed')
  return body as T
}

export function setSessionToken(value: string) {
  token = value
}
