<script lang="ts">
  import { onMount } from 'svelte'
  import { getKeys } from '../lib/api'
  import EmptyState from '../lib/components/EmptyState.svelte'

  let keys: any[] = $state([])
  let loading = $state(true)

  onMount(async () => {
    try {
      const res: any = await getKeys()
      keys = res.keys
    } catch {}
    loading = false
  })
</script>

<h2>API Keys</h2>

{#if loading}
  <p>Loading...</p>
{:else if keys.length === 0}
  <EmptyState message="No API keys configured" />
{:else}
  <table>
    <thead>
      <tr>
        <th>Key ID</th>
        <th>Total Requests</th>
        <th>Total Cost</th>
      </tr>
    </thead>
    <tbody>
      {#each keys as k}
        <tr>
          <td>{k.key_id}</td>
          <td>{k.total_requests}</td>
          <td>${k.total_cost_usd.toFixed(4)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}

<style>
  h2 { margin: 0 0 20px; font-size: 22px; }
  table { width: 100%; border-collapse: collapse; }
  th, td { padding: 10px 12px; text-align: left; border-bottom: 1px solid #334155; }
  th { color: #94a3b8; font-size: 13px; font-weight: 500; }
</style>
