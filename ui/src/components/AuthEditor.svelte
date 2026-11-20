<script lang="ts">
  import { isLiteral } from "../lib/secrets";
  import { app } from "../lib/state.svelte";
  import type { Auth, SavedRequest } from "../lib/types";

  let { req }: { req: SavedRequest } = $props();

  function setKind(kind: Auth["kind"]) {
    const prev = req.spec.auth;
    const username = "username" in prev ? prev.username : "";
    const password = "password" in prev ? prev.password : "";
    req.spec.auth =
      kind === "none"
        ? { kind }
        : kind === "basic"
          ? { kind, username, password }
          : { kind, username, password, passwordType: "PasswordText", timestampTtlSecs: 0 };
    app.markDirty();
  }

  function moveToSecret() {
    const auth = req.spec.auth;
    if (auth.kind === "none") return;
    app.secretPrompt = {
      suggestedName: "password",
      value: auth.password,
      apply: (placeholder) => {
        auth.password = placeholder;
        app.markDirty();
      },
    };
  }
</script>

<div class="form">
  <label>
    <span>Type</span>
    <select value={req.spec.auth.kind} onchange={(e) => setKind(e.currentTarget.value as Auth["kind"])}>
      <option value="none">No auth</option>
      <option value="basic">Basic (HTTP header)</option>
      <option value="wsSecurity">WS-Security UsernameToken</option>
    </select>
  </label>

  {#if req.spec.auth.kind !== "none"}
    {@const auth = req.spec.auth}
    <label>
      <span>Username</span>
      <input bind:value={auth.username} oninput={() => app.markDirty()} autocomplete="off" spellcheck="false" />
    </label>
    <label>
      <span>Password</span>
      <input
        type={isLiteral(auth.password) ? "password" : "text"}
        bind:value={auth.password}
        oninput={() => app.markDirty()}
        autocomplete="off"
        spellcheck="false"
        placeholder={"{{password}}"}
      />
    </label>
    {#if isLiteral(auth.password)}
      <div class="literal">
        <span>🔒 This password would be saved in the project.</span>
        <button class="primary small" onclick={moveToSecret}>Move to secret variable</button>
      </div>
    {/if}
    {#if auth.kind === "wsSecurity"}
      <label>
        <span>Password type</span>
        <select bind:value={auth.passwordType} onchange={() => app.markDirty()}>
          <option value="PasswordText">PasswordText</option>
          <option value="PasswordDigest">PasswordDigest</option>
        </select>
      </label>
      <label>
        <span>Timestamp TTL (s)</span>
        <input type="number" min="0" bind:value={auth.timestampTtlSecs} oninput={() => app.markDirty()} />
        <small>0 = no wsu:Timestamp</small>
      </label>
      <p class="hint">
        The <code>wsse:Security</code> header (with nonce and created time) is added when the request is sent. See
        the <b>Request</b> tab of the response to inspect it.
      </p>
    {/if}
  {/if}
  <p class="hint">
    Username and password accept <code>{"{{variables}}"}</code>. Secret variables are stored in the OS keychain,
    never in the project file.
  </p>
</div>

<style>
  .form {
    padding: 14px 16px;
    display: grid;
    gap: 10px;
    max-width: 520px;
    overflow: auto;
  }
  label {
    display: grid;
    grid-template-columns: 140px 1fr;
    align-items: center;
    gap: 4px 10px;
  }
  label span {
    color: var(--muted);
  }
  small {
    grid-column: 2;
    color: var(--muted);
  }
  .hint {
    color: var(--muted);
    margin: 4px 0 0;
    line-height: 1.5;
  }
  .literal {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    margin-left: 150px;
    padding: 6px 8px;
    border-radius: 6px;
    background: color-mix(in srgb, var(--warn) 13%, transparent);
    font-size: 12px;
  }
  .primary.small {
    padding: 3px 10px;
    font-size: 12px;
    white-space: nowrap;
  }
</style>
