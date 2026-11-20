<script lang="ts">
  import { app } from "../lib/state.svelte";

  let { id }: { id: string } = $props();
  const src = $derived(app.request(id));

  // Suggest "<name> copy", then "<name> copy 2", ... until unused.
  function suggest() {
    const s = app.request(id);
    if (!s) return "";
    let n = `${s.name} copy`;
    for (let i = 2; app.nameError(s, n); i++) n = `${s.name} copy ${i}`;
    return n;
  }

  let name = $state(suggest());
  let touched = $state(false);
  const error = $derived(src ? app.nameError(src, name) : "Request no longer exists");

  const close = () => (app.duplicating = null);

  function submit(e: Event) {
    e.preventDefault();
    touched = true;
    if (error || !src) return;
    app.duplicateRequest(id, name);
    close();
  }

  function autofocus(n: HTMLInputElement) {
    n.focus();
    n.select();
  }
</script>

<div class="backdrop" role="presentation" onclick={close}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label="Duplicate request"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.key === "Escape" && close()}
  >
    <form onsubmit={submit}>
      <h3>Duplicate request</h3>
      {#if src}
        <p class="sub">
          Copy <b>{src.operationName} · {src.name}</b> with its body, headers, auth and assertions.
        </p>
      {/if}
      <label for="dup-name">New name</label>
      <input id="dup-name" use:autofocus bind:value={name} oninput={() => (touched = true)} spellcheck="false" />
      {#if touched && error}<div class="error">{error}</div>{/if}
      <div class="actions">
        <button type="button" class="secondary" onclick={close}>Cancel</button>
        <button type="submit" class="primary" disabled={!!error}>Duplicate</button>
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
    width: min(440px, 90vw);
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
    margin: 0 0 14px;
    line-height: 1.5;
  }
  label {
    display: block;
    margin-bottom: 6px;
    color: var(--muted);
  }
  input {
    width: 100%;
  }
  .error {
    color: var(--danger);
    font-size: 12px;
    margin-top: 6px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }
</style>
