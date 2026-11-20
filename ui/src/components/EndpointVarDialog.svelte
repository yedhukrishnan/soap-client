<script lang="ts">
  import { untrack } from "svelte";
  import { app, VAR_NAME, type Scope } from "../lib/state.svelte";

  let { reqId }: { reqId: string } = $props();
  const req = $derived(app.request(reqId));
  const url = $derived(req?.spec.endpoint.trim() ?? "");
  const sameUrl = $derived(app.project.requests.filter((r) => r.spec.endpoint.trim() === url).length);

  let name = $state(untrack(() => (req ? app.suggestEndpointVarName(req) : "serviceUrl")));
  let scope = $state<Scope>(
    untrack(() => app.project.activeEnvironment ?? app.project.environments[0]?.id ?? "project"),
  );
  let applyToAll = $state(true);
  let error = $state("");

  const invalid = $derived(VAR_NAME.test(name.trim()) ? "" : "Letters, digits, _ . - only; must not start with a digit");
  const existing = $derived(app.varsOf(scope).find((v) => v.name === name.trim()));
  const otherEnvs = $derived(app.project.environments.filter((e) => e.id !== scope));

  const close = () => (app.endpointPrompt = null);

  function submit(e: Event) {
    e.preventDefault();
    if (invalid) return;
    try {
      app.endpointToVariable(reqId, name.trim(), scope, applyToAll && sameUrl > 1);
      if (scope !== "project" && app.project.activeEnvironment !== scope) app.setActiveEnvironment(scope);
      close();
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    }
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
    aria-label="Use a variable for the endpoint"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.key === "Escape" && close()}
  >
    <form onsubmit={submit}>
      <h3>Endpoint per environment</h3>
      <p class="sub">
        Store this URL in a variable so each environment (e.g. each supplier) can point the same requests at its own
        server.
      </p>
      <div class="url" title={url}>{url}</div>

      <label>
        <span>Variable name</span>
        <input use:autofocus bind:value={name} spellcheck="false" />
      </label>
      <label>
        <span>Store in</span>
        <select bind:value={scope}>
          {#each app.project.environments as env (env.id)}
            <option value={env.id}>Environment: {env.name}</option>
          {/each}
          <option value="project">Project (default for all environments)</option>
        </select>
      </label>
      {#if sameUrl > 1}
        <label class="check">
          <input type="checkbox" bind:checked={applyToAll} />
          Update all {sameUrl} requests that use this URL
        </label>
      {/if}

      {#if invalid}
        <div class="note err">{invalid}</div>
      {:else if existing}
        <div class="note">Replaces the current value of <code>{existing.name}</code> in {app.scopeName(scope)}.</div>
      {/if}
      {#if error}<div class="note err">{error}</div>{/if}

      {#if !app.project.environments.length}
        <div class="note">
          Tip: create one environment per supplier in <b>Variables &amp; Environments</b> and give each its own
          <code>{`{{${name.trim() || "url"}}}`}</code> value.
        </div>
      {:else if scope !== "project" && otherEnvs.length}
        <div class="note">
          Then set <code>{name.trim() || "url"}</code> in {otherEnvs.map((e) => e.name).join(", ")} via
          <b>Variables &amp; Environments</b>.
        </div>
      {/if}

      <div class="actions">
        <button type="button" class="secondary" onclick={close}>Cancel</button>
        <button type="submit" class="primary" disabled={!!invalid}>Use variable</button>
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
    padding-top: 13vh;
    z-index: 60;
  }
  .dialog {
    width: min(520px, 90vw);
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
    margin: 0 0 10px;
    line-height: 1.5;
  }
  .url {
    font-family: var(--mono);
    font-size: 12px;
    padding: 6px 8px;
    background: var(--hover);
    border-radius: 6px;
    margin-bottom: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
  label.check {
    display: flex;
    gap: 8px;
    margin-left: 120px;
  }
  .note {
    font-size: 12px;
    color: var(--muted);
    margin-top: 6px;
    line-height: 1.5;
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
