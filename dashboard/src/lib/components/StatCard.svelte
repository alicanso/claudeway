<script lang="ts">
  let { label, value, subtitle = '' }: { label: string; value: string | number; subtitle?: string } = $props()
</script>

<div class="card">
  <div class="label">{label}</div>
  <div class="value">{value}</div>
  {#if subtitle}
    <div class="subtitle">{subtitle}</div>
  {/if}
</div>

<style>
  .card {
    background: #1e293b;
    border: 1px solid #334155;
    border-radius: 8px;
    padding: 20px;
  }
  .label {
    font-size: 13px;
    color: #94a3b8;
    margin-bottom: 4px;
  }
  .value {
    font-size: 28px;
    font-weight: 700;
    color: #f1f5f9;
  }
  .subtitle {
    font-size: 12px;
    color: #64748b;
    margin-top: 4px;
  }
</style>
