<script lang="ts">
  import { onMount, onDestroy } from 'svelte'
  import { getLogs } from '../lib/api'
  import EmptyState from '../lib/components/EmptyState.svelte'

  let entries: any[] = $state([])
  let loading = $state(true)
  let keyFilter = $state('')
  let pollInterval: ReturnType<typeof setInterval>

  onMount(async () => {
    await load()
    pollInterval = setInterval(poll, 5000)
  })

  onDestroy(() => {
    if (pollInterval) clearInterval(pollInterval)
  })

  async function load() {
    loading = true
    try {
      const res: any = await getLogs({ key_id: keyFilter || undefined, limit: 100 })
      entries = res.entries
    } catch {}
    loading = false
  }

  async function poll() {
    if (entries.length === 0) return
    const lastTs = entries[0]?.timestamp
    if (!lastTs) return
    try {
      const res: any = await getLogs({ after: lastTs, key_id: keyFilter || undefined })
      if (res.entries.length > 0) {
        entries = [...res.entries, ...entries].slice(0, 200)
      }
    } catch {}
  }

  function levelColor(level: string): string {
    switch (level?.toUpperCase()) {
      case 'ERROR': return '#f87171'
      case 'WARN': return '#fbbf24'
      default: return '#4ade80'
    }
  }
</script>

<h2>Logs</h2>

<div class="filters">
  <input placeholder="Filter by key ID" bind:value={keyFilter} onchange={load} />
</div>

{#if loading}
  <p>Loading...</p>
{:else if entries.length === 0}
  <EmptyState message="No log entries" />
{:else}
  <div class="log-list">
    {#each entries as entry}
      <div class="log-entry">
        <span class="level" style="color: {levelColor(entry.level)}">{entry.level}</span>
        <span class="timestamp">{entry.timestamp}</span>
        <span class="key">{entry.key_id || '—'}</span>
        <span class="message">{entry.message}</span>
        {#if entry.cost_usd}
          <span class="cost">${entry.cost_usd.toFixed(4)}</span>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  h2 { margin: 0 0 20px; font-size: 22px; }
  .filters { margin-bottom: 16px; }
  .filters input {
    padding: 8px 12px; background: #1e293b; border: 1px solid #334155;
    border-radius: 6px; color: #e2e8f0; font-size: 13px; width: 240px;
  }
  .log-list { display: flex; flex-direction: column; gap: 2px; }
  .log-entry {
    display: flex; gap: 12px; padding: 8px 12px; background: #1e293b;
    border-radius: 4px; font-size: 13px; font-family: monospace;
  }
  .level { font-weight: 600; min-width: 48px; }
  .timestamp { color: #64748b; min-width: 200px; }
  .key { color: #94a3b8; min-width: 80px; }
  .message { flex: 1; color: #cbd5e1; }
  .cost { color: #fbbf24; }
</style>
