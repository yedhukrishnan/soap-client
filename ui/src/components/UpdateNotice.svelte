<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import type { UpdateStatus } from "../lib/types";

  let { status }: { status: UpdateStatus } = $props();

  const close = () => (app.updateStatus = null);

  async function download() {
    close();
    await api.updateOpen().catch(() => {});
  }

  async function skip(version: string) {
    close();
    await api.updateSkip(version).catch(() => {});
  }
</script>

<div
  class="notice"
  class:security={status.kind === "available" && status.update.security}
  role="status"
  aria-live="polite"
>
  {#if status.kind === "available"}
    {@const u = status.update}
    <h4>{u.security ? "Security update available" : "Update available"}</h4>
    <p>
      SOAP Client {u.version} is available.
      {#if u.security}It fixes security issues, so please update soon.{/if}
    </p>
    <div class="actions">
      {#if !u.security}
        <button class="link" onclick={() => skip(u.version)}>Skip This Version</button>
      {/if}
      <button class="secondary" onclick={close}>Later</button>
      <button class="primary" onclick={download}>Download</button>
    </div>
  {:else if status.kind === "current"}
    <h4>You're up to date</h4>
    <p>SOAP Client {status.version} is the latest version.</p>
    <div class="actions"><button class="secondary" onclick={close}>OK</button></div>
  {:else}
    <h4>Couldn't check for updates</h4>
    <p class="error">{status.message}</p>
    <div class="actions"><button class="secondary" onclick={close}>OK</button></div>
  {/if}
</div>

<style>
  .notice {
    position: fixed;
    right: 16px;
    bottom: 16px;
    z-index: 50;
    width: 320px;
    padding: 14px 16px 12px;
    background: var(--bg-elev);
    color: var(--fg);
    border: 1px solid var(--border-strong);
    border-left: 4px solid var(--accent);
    border-radius: 8px;
    box-shadow: var(--shadow);
  }
  .notice.security {
    border-left-color: var(--danger);
  }
  h4 {
    margin: 0 0 6px;
  }
  .security h4 {
    color: var(--danger);
  }
  p {
    margin: 0;
    color: var(--muted);
    line-height: 1.4;
  }
  .error {
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    align-items: center;
    margin-top: 12px;
  }
  .link {
    margin-right: auto;
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
    font-size: 12px;
    cursor: pointer;
  }
  .link:hover {
    color: var(--fg);
    text-decoration: underline;
  }
</style>
