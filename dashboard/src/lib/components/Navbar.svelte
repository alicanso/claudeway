<script lang="ts">
  import { isAuthenticated } from '../stores'
  import { link } from 'svelte-spa-router'

  function logout() {
    isAuthenticated.set(false)
  }
</script>

<nav>
  <div class="brand">Claudeway</div>
  <div class="links">
    <a href="/" use:link>Overview</a>
    <a href="/sessions" use:link>Sessions</a>
    <a href="/logs" use:link>Logs</a>
    <a href="/keys" use:link>Keys</a>
    <a href="/costs" use:link>Costs</a>
  </div>
  <button onclick={logout}>Logout</button>
</nav>

<style>
  nav {
    display: flex;
    align-items: center;
    padding: 12px 24px;
    background: #1e293b;
    border-bottom: 1px solid #334155;
  }
  .brand {
    font-weight: 700;
    font-size: 18px;
    color: #38bdf8;
    margin-right: 32px;
  }
  .links {
    display: flex;
    gap: 16px;
    flex: 1;
  }
  .links a {
    color: #94a3b8;
    text-decoration: none;
    padding: 4px 8px;
    border-radius: 4px;
  }
  .links a:hover {
    color: #e2e8f0;
    background: #334155;
  }
  button {
    background: none;
    border: 1px solid #475569;
    color: #94a3b8;
    padding: 6px 12px;
    border-radius: 4px;
    cursor: pointer;
  }
  button:hover {
    color: #e2e8f0;
    border-color: #64748b;
  }
</style>
