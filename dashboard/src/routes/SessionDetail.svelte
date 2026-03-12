<script lang="ts">
  import { onMount } from 'svelte'
  import { getSessionDetail } from '../lib/api'
  import StatCard from '../lib/components/StatCard.svelte'

  let { params = { id: '' } }: { params?: { id: string } } = $props()

  let session: any = $state(null)

  onMount(async () => {
    try {
      session = await getSessionDetail(params.id)
    } catch {}
  })
</script>

<h2>Session Detail</h2>

{#if session}
  <div class="grid">
    <StatCard label="Tasks" value={session.task_count} />
    <StatCard label="Cost" value={'$' + session.cost_usd.toFixed(4)} />
    <StatCard label="Input Tokens" value={session.tokens.input} />
    <StatCard label="Output Tokens" value={session.tokens.output} />
  </div>

  <div class="meta">
    <p><strong>ID:</strong> {session.session_id}</p>
    <p><strong>Model:</strong> {session.model || '—'}</p>
    <p><strong>Key:</strong> {session.key_id}</p>
    <p><strong>Workdir:</strong> {session.workdir}</p>
    <p><strong>Created:</strong> {new Date(session.created_at).toLocaleString()}</p>
    <p><strong>Last Used:</strong> {new Date(session.last_used).toLocaleString()}</p>
  </div>
{:else}
  <p>Loading...</p>
{/if}

<style>
  h2 { margin: 0 0 20px; font-size: 22px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 16px; margin-bottom: 24px; }
  .meta { background: #1e293b; border: 1px solid #334155; border-radius: 8px; padding: 20px; }
  .meta p { margin: 8px 0; font-size: 14px; }
  .meta strong { color: #94a3b8; }
</style>
