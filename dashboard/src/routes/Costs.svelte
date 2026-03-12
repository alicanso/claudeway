<script lang="ts">
  import { onMount } from 'svelte'
  import { getCosts } from '../lib/api'
  import EmptyState from '../lib/components/EmptyState.svelte'
  import Chart from 'chart.js/auto'

  let data: any = $state(null)
  let groupBy = $state('daily')
  let loading = $state(true)
  let chartCanvas: HTMLCanvasElement
  let chartInstance: Chart | null = null
  let modelCanvas: HTMLCanvasElement
  let keyCanvas: HTMLCanvasElement
  let modelChart: Chart | null = null
  let keyChart: Chart | null = null

  onMount(() => load())

  async function load() {
    loading = true
    try {
      const res: any = await getCosts(groupBy)
      data = res.data
      renderChart()
    } catch {}
    loading = false
  }

  function renderChart() {
    if (!chartCanvas || !data || data.length === 0) return
    if (chartInstance) chartInstance.destroy()
    chartInstance = new Chart(chartCanvas, {
      type: 'bar',
      data: {
        labels: data.map((d: any) => d.period),
        datasets: [{
          label: 'Cost (USD)',
          data: data.map((d: any) => d.cost_usd),
          backgroundColor: '#38bdf8',
          borderRadius: 4,
        }],
      },
      options: {
        responsive: true,
        plugins: { legend: { labels: { color: '#94a3b8' } } },
        scales: {
          x: { ticks: { color: '#64748b' }, grid: { color: '#1e293b' } },
          y: { ticks: { color: '#64748b' }, grid: { color: '#1e293b' } },
        },
      },
    })
    renderModelChart()
    renderKeyChart()
  }

  function renderModelChart() {
    if (!modelCanvas || !data || data.length === 0) return
    if (modelChart) modelChart.destroy()
    const models = new Set<string>()
    data.forEach((d: any) => d.by_model?.forEach((m: any) => models.add(m.model)))
    const modelList = [...models]
    const colors = ['#38bdf8', '#a78bfa', '#4ade80', '#fbbf24', '#f87171']
    modelChart = new Chart(modelCanvas, {
      type: 'bar',
      data: {
        labels: data.map((d: any) => d.period),
        datasets: modelList.map((model, i) => ({
          label: model,
          data: data.map((d: any) => {
            const m = d.by_model?.find((b: any) => b.model === model)
            return m ? m.cost_usd : 0
          }),
          backgroundColor: colors[i % colors.length],
        })),
      },
      options: {
        responsive: true,
        plugins: { legend: { labels: { color: '#94a3b8' } } },
        scales: {
          x: { stacked: true, ticks: { color: '#64748b' }, grid: { color: '#1e293b' } },
          y: { stacked: true, ticks: { color: '#64748b' }, grid: { color: '#1e293b' } },
        },
      },
    })
  }

  function renderKeyChart() {
    if (!keyCanvas || !data || data.length === 0) return
    if (keyChart) keyChart.destroy()
    const keyTotals: Record<string, number> = {}
    data.forEach((d: any) => d.by_key?.forEach((k: any) => {
      keyTotals[k.key_id] = (keyTotals[k.key_id] || 0) + k.cost_usd
    }))
    const keys = Object.keys(keyTotals)
    const colors = ['#38bdf8', '#a78bfa', '#4ade80', '#fbbf24', '#f87171']
    keyChart = new Chart(keyCanvas, {
      type: 'bar',
      data: {
        labels: keys,
        datasets: [{
          label: 'Total Cost (USD)',
          data: keys.map(k => keyTotals[k]),
          backgroundColor: keys.map((_, i) => colors[i % colors.length]),
          borderRadius: 4,
        }],
      },
      options: {
        responsive: true,
        indexAxis: 'y',
        plugins: { legend: { display: false } },
        scales: {
          x: { ticks: { color: '#64748b' }, grid: { color: '#1e293b' } },
          y: { ticks: { color: '#64748b' }, grid: { color: '#1e293b' } },
        },
      },
    })
  }
</script>

<h2>Cost Analytics</h2>

<div class="toggle">
  {#each ['daily', 'weekly', 'monthly'] as g}
    <button class:active={groupBy === g} onclick={() => { groupBy = g; load() }}>{g}</button>
  {/each}
</div>

{#if loading}
  <p>Loading...</p>
{:else if !data || data.length === 0}
  <EmptyState message="No cost data yet" />
{:else}
  <h3>Total Cost by Period</h3>
  <div class="chart-container">
    <canvas bind:this={chartCanvas}></canvas>
  </div>

  <h3>Cost by Model (stacked)</h3>
  <div class="chart-container">
    <canvas bind:this={modelCanvas}></canvas>
  </div>

  <h3>Cost by Key</h3>
  <div class="chart-container">
    <canvas bind:this={keyCanvas}></canvas>
  </div>

  <table>
    <thead>
      <tr>
        <th>Period</th>
        <th>Requests</th>
        <th>Cost</th>
      </tr>
    </thead>
    <tbody>
      {#each data as d}
        <tr>
          <td>{d.period}</td>
          <td>{d.request_count}</td>
          <td>${d.cost_usd.toFixed(4)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}

<style>
  h2 { margin: 0 0 20px; font-size: 22px; }
  h3 { margin: 20px 0 8px; font-size: 15px; color: #94a3b8; }
  .toggle { display: flex; gap: 8px; margin-bottom: 20px; }
  .toggle button {
    padding: 6px 16px; background: #1e293b; border: 1px solid #334155;
    color: #94a3b8; border-radius: 4px; cursor: pointer; text-transform: capitalize;
  }
  .toggle button.active { background: #38bdf8; color: #0f172a; border-color: #38bdf8; }
  .chart-container { background: #1e293b; border-radius: 8px; padding: 20px; margin-bottom: 20px; }
  table { width: 100%; border-collapse: collapse; }
  th, td { padding: 10px 12px; text-align: left; border-bottom: 1px solid #334155; }
  th { color: #94a3b8; font-size: 13px; font-weight: 500; }
</style>
