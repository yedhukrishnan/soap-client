<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { app } from "../lib/state.svelte";

  let source = $state("");
  let busy = $state(false);
  let error = $state("");

  async function browse() {
    const p = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "WSDL", extensions: ["wsdl", "xml", "svc", "asmx"] }, { name: "All files", extensions: ["*"] }],
    });
    if (p) source = p;
  }

  async function submit(e: Event) {
    e.preventDefault();
    if (!source.trim() || busy) return;
    busy = true;
    error = "";
    try {
      await app.importWsdl(source.trim());
      app.importOpen = false;
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  function autofocus(n: HTMLElement) {
    n.focus();
  }
</script>

<div class="backdrop" role="presentation" onclick={() => !busy && (app.importOpen = false)}>
  <div
    class="dialog"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.key === "Escape" && !busy && (app.importOpen = false)}
    role="dialog"
    aria-label="Import WSDL"
    aria-modal="true"
    tabindex="-1"
  >
  <form onsubmit={submit}>
    <h3>Import WSDL</h3>
    <p class="sub">Enter a URL (e.g. <code>https://host/service?wsdl</code>) or choose a local file.</p>
    <div class="row">
      <input use:autofocus bind:value={source} placeholder="URL or file path" spellcheck="false" disabled={busy} />
      <button type="button" class="secondary" onclick={browse} disabled={busy}>Browse…</button>
    </div>
    <p class="sub small">Supports WSDL 1.1. Schemas and WSDLs it imports are loaded too.</p>
    {#if error}<div class="error">{error}</div>{/if}
    <div class="actions">
      <button type="button" class="secondary" onclick={() => (app.importOpen = false)} disabled={busy}>Cancel</button>
      <button type="submit" class="primary" disabled={busy || !source.trim()}>
        {#if busy}<span class="spinner"></span>{/if} Import
      </button>
    </div>
  </form>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 15vh;
    z-index: 50;
  }
  .dialog {
    width: min(560px, 90vw);
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow);
    padding: 18px 20px;
  }
  h3 {
    margin: 0 0 4px;
  }
  .sub {
    color: var(--muted);
    margin: 0 0 12px;
  }
  .small {
    font-size: 12px;
    margin-top: 8px;
  }
  .row {
    display: flex;
    gap: 8px;
  }
  .row input {
    flex: 1;
    font-family: var(--mono);
    font-size: 12.5px;
  }
  .error {
    color: var(--danger);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    border-radius: 6px;
    padding: 8px 10px;
    margin-top: 10px;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }
</style>
