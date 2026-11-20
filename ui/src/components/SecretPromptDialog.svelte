<script lang="ts">
  import { untrack } from "svelte";
  import { app, VAR_NAME, type Scope, type SecretPrompt } from "../lib/state.svelte";

  let { prompt }: { prompt: SecretPrompt } = $props();

  let scope = $state<Scope>(app.project.activeEnvironment ?? "project");
  let name = $state(untrack(() => prompt.suggestedName));
  let busy = $state(false);
  let failure = $state("");

  const existing = $derived(app.varsOf(scope).find((v) => v.name === name.trim()));
  const invalid = $derived(VAR_NAME.test(name.trim()) ? "" : "Letters, digits, _ . - only; must not start with a digit");

  const close = () => (app.secretPrompt = null);

  async function submit(e: Event) {
    e.preventDefault();
    if (invalid || busy) return;
    busy = true;
    failure = "";
    try {
      await app.storeSecretVariable(scope, name.trim(), prompt.value);
      prompt.apply(`{{${name.trim()}}}`);
      close();
    } catch (err) {
      failure = err instanceof Error ? err.message : String(err);
    } finally {
      busy = false;
    }
  }

  function autofocus(n: HTMLInputElement) {
    n.focus();
    n.select();
  }
</script>

<div class="backdrop" role="presentation" onclick={() => !busy && close()}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label="Move to secret variable"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.key === "Escape" && !busy && close()}
  >
    <form onsubmit={submit}>
      <h3>🔒 Move to secret variable</h3>
      <p class="sub">
        The value ({prompt.value.length} characters) is stored in
        {app.secretBackend?.persistent ? (app.secretBackend.name ?? "the keychain") : "memory for this session"} and
        replaced with a <code>{"{{placeholder}}"}</code>. The project file never contains it.
      </p>
      <label>
        <span>Variable name</span>
        <input use:autofocus bind:value={name} spellcheck="false" />
      </label>
      <label>
        <span>Store in</span>
        <select bind:value={scope}>
          <option value="project">Project (all environments)</option>
          {#each app.project.environments as env (env.id)}
            <option value={env.id}>Environment: {env.name}</option>
          {/each}
        </select>
      </label>
      {#if invalid}
        <div class="note err">{invalid}</div>
      {:else if existing}
        <div class="note">
          {existing.secret ? "Replaces the stored value of" : "Converts the existing plain variable"}
          <code>{existing.name}</code> in {app.scopeName(scope)}.
        </div>
      {/if}
      {#if failure}<div class="note err">{failure}</div>{/if}
      <div class="actions">
        <button type="button" class="secondary" onclick={close} disabled={busy}>Cancel</button>
        <button type="submit" class="primary" disabled={!!invalid || busy}>
          {#if busy}<span class="spinner"></span>{/if} Move to secret
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
    z-index: 60;
  }
  .dialog {
    width: min(480px, 90vw);
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
    display: grid;
    grid-template-columns: 110px 1fr;
    align-items: center;
    gap: 10px;
    margin-bottom: 10px;
  }
  label span {
    color: var(--muted);
  }
  .note {
    font-size: 12px;
    color: var(--muted);
  }
  .note.err {
    color: var(--danger);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }
</style>
