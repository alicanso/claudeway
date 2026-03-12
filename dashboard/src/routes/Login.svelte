<script lang="ts">
  import { login } from '../lib/api'

  let key = $state('')
  let error = $state('')
  let loading = $state(false)

  async function handleSubmit(e: Event) {
    e.preventDefault()
    error = ''
    loading = true
    try {
      const ok = await login(key)
      if (!ok) {
        error = 'Invalid admin key'
      }
    } catch (err) {
      error = 'Connection failed'
    } finally {
      loading = false
    }
  }
</script>

<div class="login-container">
  <div class="login-box">
    <h1>Claudeway</h1>
    <p>Admin Dashboard</p>
    <form onsubmit={handleSubmit}>
      <input
        type="password"
        placeholder="Admin API Key"
        bind:value={key}
        disabled={loading}
      />
      <button type="submit" disabled={loading || !key}>
        {loading ? 'Logging in...' : 'Login'}
      </button>
      {#if error}
        <div class="error">{error}</div>
      {/if}
    </form>
  </div>
</div>

<style>
  .login-container {
    display: flex;
    justify-content: center;
    align-items: center;
    min-height: 100vh;
  }
  .login-box {
    background: #1e293b;
    border: 1px solid #334155;
    border-radius: 12px;
    padding: 40px;
    width: 360px;
    text-align: center;
  }
  h1 {
    color: #38bdf8;
    margin: 0 0 4px;
  }
  p {
    color: #94a3b8;
    margin: 0 0 24px;
  }
  input {
    width: 100%;
    padding: 10px 12px;
    background: #0f172a;
    border: 1px solid #334155;
    border-radius: 6px;
    color: #e2e8f0;
    font-size: 14px;
    margin-bottom: 12px;
    box-sizing: border-box;
  }
  button {
    width: 100%;
    padding: 10px;
    background: #38bdf8;
    color: #0f172a;
    border: none;
    border-radius: 6px;
    font-weight: 600;
    cursor: pointer;
    font-size: 14px;
  }
  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .error {
    color: #f87171;
    font-size: 13px;
    margin-top: 12px;
  }
</style>
