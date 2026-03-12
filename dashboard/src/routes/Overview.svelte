<script lang="ts">
  import { onMount } from 'svelte'
  import { getOverview, getCosts } from '../lib/api'
  import StatCard from '../lib/components/StatCard.svelte'
  import Chart from 'chart.js/auto'

  let data: any = $state(null)
  let error = $state('')
  let lineCanvas: HTMLCanvasElement
  let pieCanvas: HTMLCanvasElement
  let lineChart: Chart | null = null
  let pieChart: Chart | null = null

  onMount(async () => {
    try {
      data = await getOverview()
      const costs: any = await getCosts('daily')
      renderLineChart(costs.data)
      renderPieChart(data.models_breakdown)
    } catch (e: any) {
      error = e.message
    }
  })

  function formatUptime(secs: number): string {
    const h = Math.floor(secs / 3600)
    const m = Math.floor((secs % 3600) / 60)
    return `${h}h ${m}m`
  }

  function formatCost(usd: number): string {
    return `$${usd.toFixed(4)}`
  }

  function renderLineChart(costData: any[]) {
    if (!lineCanvas || !costData?.length) return
    if (lineChart) lineChart.destroy()
    const recent = costData.slice(-30)
    lineChart = new Chart(lineCanvas, {
      type: 'line',
      data: {
        labels: recent.map((d: any) => d.period),
        datasets: [
          {
            label: 'Cost (USD)',
            data: recent.map((d: any) => d.cost_usd),
            borderColor: '#38bdf8',
            tension: 0.3,
          },
          {
            label: 'Requests',
            data: recent.map((d: any) => d.request_count),
            borderColor: '#a78bfa',
            tension: 0.3,
            yAxisID: 'y1',
          },
        ],
      },
      options: {
        responsive: true,
        plugins: { legend: { labels: { color: '#94a3b8' } } },
        scales: {
          x: { ticks: { color: '#64748b' }, grid: { color: '#1e293b' } },
          y: { ticks: { color: '#64748b' }, grid: { color: '#1e293b' }, position: 'left' },
          y1: { ticks: { color: '#64748b' }, grid: { display: false }, position: 'right' },
        },
      },
    })
  }

  function renderPieChart(breakdown: any[]) {
    if (!pieCanvas || !breakdown?.length) return
    if (pieChart) pieChart.destroy()
    pieChart = new Chart(pieCanvas, {
      type: 'doughnut',
      data: {
        labels: breakdown.map((b: any) => b.model),
        datasets: [{
          data: breakdown.map((b: any) => b.request_count),
          backgroundColor: ['#38bdf8', '#a78bfa', '#4ade80', '#fbbf24', '#f87171'],
        }],
      },
      options: {
        responsive: true,
        plugins: { legend: { labels: { color: '#94a3b8' } } },
      },
    })
  }
</script>

<h2>Overview</h2>

{#if error}
  <div class="error">{error}</div>
{:else if data}
  <div class="grid">
    <StatCard label="Uptime" value={formatUptime(data.uptime_secs)} />
    <StatCard label="Total Requests" value={data.total_requests} />
    <StatCard label="Active Sessions" value={data.active_sessions} />
    <StatCard label="Total Cost" value={formatCost(data.total_cost_usd)} />
  </div>

  <div class="charts">
    <div class="chart-box">
      <h3>Daily Requests & Cost (last 30 days)</h3>
      <canvas bind:this={lineCanvas}></canvas>
    </div>
    <div class="chart-box small">
      <h3>Model Usage</h3>
      <canvas bind:this={pieCanvas}></canvas>
    </div>
  </div>
{:else}
  <p>Loading...</p>
{/if}

<style>
  h2 { margin: 0 0 20px; font-size: 22px; }
  h3 { margin: 0 0 12px; font-size: 15px; color: #94a3b8; }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 16px;
    margin-bottom: 24px;
  }
  .charts { display: grid; grid-template-columns: 2fr 1fr; gap: 16px; }
  .chart-box { background: #1e293b; border-radius: 8px; padding: 20px; }
  .error { color: #f87171; padding: 12px; background: #1e293b; border-radius: 8px; }
</style>
