<script lang="ts">
  import { onMount } from "svelte";
  import { secretStore } from "../lib/platform";
  import { app, VAR_NAME, type Scope } from "../lib/state.svelte";
  import type { Variable } from "../lib/types";

  let scope = $state<Scope>(app.project.activeEnvironment ?? "project");
  let error = $state("");
  let renamingEnv = $state<string | null>(null);

  const vars = $derived(app.varsOf(scope));
  const inherited = $derived(
    scope === "project" ? [] : app.project.variables.filter((p) => !vars.some((v) => v.name === p.name)),
  );
  const overrides = (name: string) => scope !== "project" && app.project.variables.some((p) => p.name === name);

  onMount(() => {
    app.refreshSecretStatus();
  });

  async function run(fn: () => Promise<unknown> | unknown) {
    error = "";
    try {
      await fn();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  /** The row just added with "+ Add variable"; its name field gets focus. */
  let justAdded = $state<Variable | null>(null);

  function addVariable() {
    const list = app.varsOf(scope);
    list.push({ name: "", value: "", secret: false });
    justAdded = list[list.length - 1];
    app.markDirty();
  }

  /** Drop rows that were added but never named (and hold no secret). */
  function pruneUnnamed() {
    let removed = false;
    for (const s of ["project", ...app.project.environments.map((e) => e.id)]) {
      const list = app.varsOf(s);
      for (let k = list.length - 1; k >= 0; k--) {
        if (!list[k].name.trim() && !list[k].secret) {
          list.splice(k, 1);
          removed = true;
        }
      }
    }
    if (removed) app.markDirty();
  }

  function selectScope(s: Scope) {
    pruneUnnamed();
    scope = s;
  }

  function focusIf(node: HTMLInputElement, focus: boolean) {
    if (focus) node.focus();
  }

  function commitName(v: Variable, input: HTMLInputElement) {
    const name = input.value.trim();
    if (name === v.name) return;
    if (!name && !v.name) return;
    if (!VAR_NAME.test(name)) {
      error = `"${name}" is not a valid name (letters, digits, _ . - ; must not start with a digit)`;
      input.value = v.name;
    } else if (vars.some((x) => x !== v && x.name === name)) {
      error = `"${name}" already exists in ${app.scopeName(scope)}`;
      input.value = v.name;
    } else {
      run(() => app.renameVariable(scope, v, name));
    }
  }

  function commitSecret(v: Variable, input: HTMLInputElement) {
    const value = input.value;
    if (!value) return;
    run(async () => {
      await app.setSecretValue(scope, v.name, value);
      input.value = "";
    });
  }

  function newEnvironment() {
    let name = "New environment";
    for (let i = 2; app.project.environments.some((e) => e.name === name); i++) name = `New environment ${i}`;
    pruneUnnamed();
    const env = app.addEnvironment(name);
    scope = env.id;
    renamingEnv = env.id;
  }

  function focusSelect(n: HTMLInputElement) {
    n.focus();
    n.select();
  }

  const close = () => {
    pruneUnnamed();
    app.varsOpen = false;
  };
</script>

<div class="backdrop" role="presentation" onclick={close}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label="Variables & Environments"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.key === "Escape" && !renamingEnv && close()}
  >
    <header>
      <h3>Variables &amp; Environments</h3>
      <button class="x" onclick={close} title="Close">×</button>
    </header>

    <div class="body">
      <nav>
        <button class="scope" class:on={scope === "project"} onclick={() => selectScope("project")}>
          Project
          <span class="sub">default for all environments</span>
        </button>
        <div class="navhead">
          Environments
          <button class="add" title="New environment" onclick={newEnvironment}>+</button>
        </div>
        {#each app.project.environments as env (env.id)}
          {#if renamingEnv === env.id}
            <input
              class="envname"
              value={env.name}
              use:focusSelect
              onkeydown={(e) => {
                if (e.key === "Enter") e.currentTarget.blur();
                if (e.key === "Escape") renamingEnv = null;
              }}
              onblur={(e) => {
                const v = e.currentTarget.value.trim();
                if (renamingEnv && v && v !== env.name) {
                  env.name = v;
                  app.markDirty();
                }
                renamingEnv = null;
              }}
            />
          {:else}
            <div class="scope env" class:on={scope === env.id}>
              <button class="envbtn" onclick={() => selectScope(env.id)} ondblclick={() => (renamingEnv = env.id)} title="Double-click to rename">
                <span class="envlabel">{env.name}</span>
                {#if app.project.activeEnvironment === env.id}
                  <span class="active" title="Active environment">●</span>
                {/if}
              </button>
              <button
                class="del"
                title="Delete environment"
                onclick={() =>
                  run(async () => {
                    if (await app.removeEnvironment(env.id)) scope = "project";
                  })}>×</button
              >
            </div>
          {/if}
        {:else}
          <p class="none">e.g. one per supplier or per stage (test / prod).</p>
        {/each}
      </nav>

      <section>
        <div class="tophead">
          <h4>{app.scopeName(scope)}</h4>
          {#if scope !== "project" && app.project.activeEnvironment === scope}<span class="activetag">● Active</span>{/if}
          {#if scope !== "project"}
            {#if app.project.activeEnvironment === scope}
              <button class="link" onclick={() => app.setActiveEnvironment(null)}>Deactivate</button>
            {:else}
              <button class="link" onclick={() => app.setActiveEnvironment(scope)}>Make active</button>
            {/if}
          {/if}
        </div>

        <table>
          <thead>
            <tr><th>Name</th><th>Value</th><th title={`Secret values are stored in your ${secretStore}`}>Secret</th><th></th></tr>
          </thead>
          <tbody>
            {#each vars as v (v)}
              {@const key = app.secretKey(scope, v.name)}
              <tr>
                <td class="name">
                  <input
                    value={v.name}
                    placeholder="name, e.g. supplierId"
                    spellcheck="false"
                    use:focusIf={v === justAdded}
                    onchange={(e) => commitName(v, e.currentTarget)}
                    onkeydown={(e) => e.key === "Enter" && e.currentTarget.blur()}
                  />
                  {#if overrides(v.name)}<span class="tag" title="Overrides the project value">overrides</span>{/if}
                </td>
                <td>
                  {#if v.secret}
                    <input
                      type="password"
                      autocomplete="off"
                      placeholder={app.secretStatus[key]
                        ? "•••••• stored — type to replace"
                        : "Not set — type a value and press Enter"}
                      class:unset={!app.secretStatus[key]}
                      onchange={(e) => commitSecret(v, e.currentTarget)}
                      onkeydown={(e) => e.key === "Enter" && e.currentTarget.blur()}
                    />
                  {:else}
                    <input bind:value={v.value} placeholder="value" oninput={() => app.markDirty()} spellcheck="false" />
                  {/if}
                </td>
                <td class="c">
                  <button
                    class="lock"
                    class:on={v.secret}
                    disabled={!v.name}
                    title={v.secret
                      ? `Secret, stored in your ${secretStore}. Click to make it plain.`
                      : "Plain, saved in the project. Click to make it secret."}
                    onclick={() => run(() => app.setSecret(scope, v, !v.secret))}>{v.secret ? "🔒" : "🔓"}</button
                  >
                </td>
                <td class="c">
                  <button class="del" title="Delete variable" onclick={() => run(() => app.removeVariable(scope, v))}>×</button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
        <button class="addvar" onclick={addVariable}>+ Add variable</button>

        {#if inherited.length}
          <p class="inherited">
            Inherited from Project:
            {#each inherited as p, i (p.name)}<code>{p.name}</code>{i < inherited.length - 1 ? ", " : ""}{/each}
          </p>
        {/if}

        {#if error}<div class="error">{error}</div>{/if}
      </section>
    </div>

    <footer>
      <span>Use <code>{"{{name}}"}</code> in the body, endpoint, SOAPAction, headers or auth fields.</span>
      {#if app.secretBackend}
        <span class:warn={!app.secretBackend.persistent}>
          {app.secretBackend.persistent
            ? `Secrets: ${app.secretBackend.name}`
            : "No keychain available: secrets are kept only until you quit"}
          {#if app.secretBackend.persistent}
            · <button class="link" onclick={() => app.showKeychainInfo()}>About secret storage</button>
          {/if}
        </span>
      {/if}
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 50;
  }
  .dialog {
    width: min(880px, 94vw);
    height: min(560px, 88vh);
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
  }
  h3 {
    margin: 0;
    flex: 1;
  }
  .x {
    font-size: 18px;
    color: var(--muted);
    padding: 0 6px;
  }
  .body {
    flex: 1;
    display: grid;
    grid-template-columns: 240px 1fr;
    min-height: 0;
  }
  nav {
    border-right: 1px solid var(--border);
    background: var(--bg-side);
    padding: 8px;
    overflow: auto;
  }
  .scope {
    display: block;
    width: 100%;
    text-align: left;
    padding: 6px 8px;
    border-radius: 6px;
  }
  .scope.on,
  .scope.env.on {
    background: var(--selected);
  }
  .scope:hover:not(.on) {
    background: var(--hover);
  }
  .scope .sub {
    display: block;
    font-size: 11px;
    color: var(--muted);
  }
  .navhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 14px 8px 4px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .add {
    font-size: 16px;
    color: var(--accent);
    padding: 0 4px;
  }
  .env {
    display: flex;
    align-items: center;
    padding: 0 4px 0 0;
  }
  .envbtn {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    text-align: left;
    padding: 6px 8px;
  }
  .envlabel {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* A dot never gets squeezed, unlike a text badge next to a long name. */
  .active {
    flex-shrink: 0;
    color: var(--ok);
    font-size: 9px;
  }
  .envname {
    width: 100%;
    margin: 2px 0;
  }
  .none {
    color: var(--muted);
    font-size: 12px;
    padding: 0 8px;
    line-height: 1.5;
  }
  section {
    padding: 12px 16px;
    overflow: auto;
  }
  .tophead {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 8px;
  }
  h4 {
    margin: 0;
  }
  .link {
    color: var(--accent);
    font-size: 12px;
  }
  .activetag {
    color: var(--ok);
    font-size: 12px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th {
    text-align: left;
    font-weight: 500;
    color: var(--muted);
    font-size: 12px;
    padding: 4px;
  }
  td {
    padding: 3px 4px;
  }
  td input {
    width: 100%;
    font-family: var(--mono);
    font-size: 12.5px;
  }
  td.name {
    width: 34%;
    position: relative;
  }
  .tag {
    position: absolute;
    right: 10px;
    top: 50%;
    transform: translateY(-50%);
    font-size: 10px;
    color: var(--warn);
    pointer-events: none;
  }
  input.unset::placeholder {
    color: var(--warn);
  }
  .c {
    width: 44px;
    text-align: center;
  }
  .lock {
    opacity: 0.45;
    font-size: 14px;
  }
  .lock.on {
    opacity: 1;
  }
  .del {
    color: var(--muted);
    padding: 0 6px;
  }
  .del:hover {
    color: var(--danger);
  }
  .addvar {
    color: var(--accent);
    margin-top: 8px;
  }
  .inherited {
    color: var(--muted);
    font-size: 12px;
    margin-top: 14px;
  }
  .error {
    color: var(--danger);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    border-radius: 6px;
    padding: 8px 10px;
    margin-top: 12px;
  }
  footer {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    padding: 10px 16px;
    border-top: 1px solid var(--border);
    color: var(--muted);
    font-size: 12px;
  }
  .warn {
    color: var(--warn);
  }
</style>
