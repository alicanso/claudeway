<script lang="ts">
  import { onMount } from 'svelte'
  import { getSessions } from '../lib/api'
  import { link } from 'svelte-spa-router'
  import EmptyState from '../lib/components/EmptyState.svelte'

  let sessions: any[] = $state([])
  let total = $state(0)
  let page = $state(1)
  let loading = $state(true)

  onMount(() => load())

  async function load() {
    loading = true
    try {
      const res: any = await getSessions(page)
      sessions = res.sessions
      total = res.total
    } catch {}
    loading = false
  }

  function formatDate(iso: string): string {
    return new Date(iso).toLocaleString()
  }
</script>

<h2>Sessions</h2>

{#if loading}
  <p>Loading...</p>
{:else if sessions.length === 0}
  <EmptyState message="No sessions yet" />
{:else}
  <table>
    <thead>
      <tr>
        <th>Session ID</th>
        <th>Model</th>
        <th>Tasks</th>
        <th>Cost</th>
        <th>Created</th>
      </tr>
    </thead>
    <tbody>
      {#each sessions as s}
        <tr>
          <td><a href="/sessions/{s.session_id}" use:link>{s.session_id.slice(0, 8)}...</a></td>
          <td>{s.model || '—'}</td>
          <td>{s.task_count}</td>
          <td>${s.cost_usd.toFixed(4)}</td>
          <td>{formatDate(s.created_at)}</td>
        </tr>
      {/each}
    </tbody>
  </table>

  <div class="pagination">
    <button onclick={() => { page--; load() }} disabled={page <= 1}>Prev</button>
    <span>Page {page}</span>
    <button onclick={() => { page++; load() }} disabled={sessions.length < 20}>Next</button>
  </div>
{/if}

<style>
  h2 { margin: 0 0 20px; font-size: 22px; }
  table { width: 100%; border-collapse: collapse; }
  th, td { padding: 10px 12px; text-align: left; border-bottom: 1px solid #334155; }
  th { color: #94a3b8; font-size: 13px; font-weight: 500; }
  td a { color: #38bdf8; text-decoration: none; }
  td a:hover { text-decoration: underline; }
  .pagination { display: flex; align-items: center; gap: 12px; margin-top: 16px; }
  .pagination button {
    padding: 6px 12px; background: #1e293b; border: 1px solid #334155;
    color: #e2e8f0; border-radius: 4px; cursor: pointer;
  }
  .pagination button:disabled { opacity: 0.4; cursor: not-allowed; }
</style>
