const BASE = '/admin'

import { isAuthenticated } from './stores'

async function request<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' },
    ...options,
  })

  if (res.status === 401) {
    window.location.hash = '#/'
    isAuthenticated.set(false)
    throw new Error('Session expired')
  }

  if (!res.ok) {
    const body = await res.json().catch(() => ({}))
    throw new Error(body.error || `HTTP ${res.status}`)
  }

  return res.json()
}

export async function login(key: string): Promise<boolean> {
  const res = await fetch(`${BASE}/login`, {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ key }),
  })
  if (res.ok) {
    isAuthenticated.set(true)
    return true
  }
  return false
}

export function getOverview() {
  return request('/overview')
}

export function getSessions(page = 1, limit = 20, model?: string) {
  const params = new URLSearchParams({ page: String(page), limit: String(limit) })
  if (model) params.set('model', model)
  return request(`/sessions?${params}`)
}

export function getSessionDetail(id: string) {
  return request(`/sessions/${id}`)
}

export function getLogs(opts: { key_id?: string; date?: string; after?: string; limit?: number } = {}) {
  const params = new URLSearchParams()
  if (opts.key_id) params.set('key_id', opts.key_id)
  if (opts.date) params.set('date', opts.date)
  if (opts.after) params.set('after', opts.after)
  if (opts.limit) params.set('limit', String(opts.limit))
  return request(`/logs?${params}`)
}

export function getKeys() {
  return request('/keys')
}

export function getCosts(groupBy = 'daily') {
  return request(`/costs?group_by=${groupBy}`)
}
