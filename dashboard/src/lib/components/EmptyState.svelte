<script lang="ts">
  let { message = 'No data yet' }: { message?: string } = $props()
</script>

<div class="empty">
  <p>{message}</p>
</div>

<style>
  .empty {
    text-align: center;
    padding: 48px;
    color: #64748b;
    font-size: 15px;
  }
</style>
