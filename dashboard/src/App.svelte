<script lang="ts">
  import Router from 'svelte-spa-router'
  import Navbar from './lib/components/Navbar.svelte'
  import Login from './routes/Login.svelte'
  import Overview from './routes/Overview.svelte'
  import Sessions from './routes/Sessions.svelte'
  import SessionDetail from './routes/SessionDetail.svelte'
  import Logs from './routes/Logs.svelte'
  import Keys from './routes/Keys.svelte'
  import Costs from './routes/Costs.svelte'
  import { isAuthenticated } from './lib/stores'

  const routes = {
    '/': Overview,
    '/sessions': Sessions,
    '/sessions/:id': SessionDetail,
    '/logs': Logs,
    '/keys': Keys,
    '/costs': Costs,
  }
</script>

{#if $isAuthenticated}
  <Navbar />
  <main>
    <Router {routes} />
  </main>
{:else}
  <Login />
{/if}

<style>
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: #0f172a;
    color: #e2e8f0;
  }
  main {
    max-width: 1200px;
    margin: 0 auto;
    padding: 24px;
  }
</style>
