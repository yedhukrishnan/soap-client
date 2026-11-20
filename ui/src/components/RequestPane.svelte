<script lang="ts">
  import { api } from "../lib/api";
  import { findLiteralSecrets, replaceLiteralSecret } from "../lib/secrets";
  import { app, isHttpUrl } from "../lib/state.svelte";
  import type { SavedRequest } from "../lib/types";
  import AssertionsEditor from "./AssertionsEditor.svelte";
  import AuthEditor from "./AuthEditor.svelte";
  import HeadersEditor from "./HeadersEditor.svelte";
  import XmlEditor from "./XmlEditor.svelte";

  let { req }: { req: SavedRequest } = $props();
  let tab = $state<"body" | "auth" | "headers" | "assertions">("body");
  let note = $state("");

  const run = $derived(app.runs[req.id]);
  const source = $derived(app.operationOf(req));
  const endpoints = $derived((source?.iface.endpoints ?? []).filter(isHttpUrl));
  /** e.g. "[Endpoint URL]" from PromoStandards WSDLs: shown as a hint, never as text. */
  const wsdlPlaceholder = $derived(source?.iface.endpoints.find((e) => !isHttpUrl(e)));
  const endpointHint = $derived(
    wsdlPlaceholder
      ? `${wsdlPlaceholder}: e.g. https://supplier.com/service or {{productDataUrl}}`
      : "https://host/service or {{serviceUrl}}",
  );
  const headerCount = $derived(req.spec.headers.filter((h) => h.enabled && h.name).length);
  const literalSecrets = $derived(findLiteralSecrets(req.spec.body));
  const templated = $derived(req.spec.endpoint.includes("{{"));
  const resolved = $derived(templated ? app.preview(req.spec.endpoint) : null);
  const scopeLabel = $derived(app.activeEnvironment ? `environment "${app.activeEnvironment.name}"` : "the project (no environment active)");

  function moveToSecret() {
    const s = literalSecrets[0];
    if (!s) return;
    app.secretPrompt = {
      suggestedName: s.local.toLowerCase() === "password" ? "password" : s.local,
      value: s.decoded,
      apply: (placeholder) => {
        req.spec.body = replaceLiteralSecret(req.spec.body, s, placeholder);
        app.markDirty();
      },
    };
  }

  function flash(msg: string) {
    note = msg;
    setTimeout(() => (note = ""), 2500);
  }

  async function format() {
    const pretty = await api.formatXml(req.spec.body);
    if (pretty == null) flash("Body is not well-formed XML");
    else {
      req.spec.body = pretty;
      app.markDirty();
    }
  }

  function resetToSample() {
    if (!source) return flash("Operation no longer exists in the WSDL");
    req.spec.body = source.op.sampleRequest;
    app.markDirty();
  }
</script>

<section class="pane">
  <div class="urlbar">
    <select
      bind:value={req.spec.soapVersion}
      onchange={() => app.markDirty()}
      title="SOAP version (sets Content-Type / SOAPAction handling)"
    >
      <option value="1.1">SOAP 1.1</option>
      <option value="1.2">SOAP 1.2</option>
    </select>
    <input
      class="endpoint"
      list="endpoints-{req.id}"
      placeholder={endpointHint}
      bind:value={req.spec.endpoint}
      oninput={() => app.markDirty()}
      spellcheck="false"
    />
    <datalist id="endpoints-{req.id}">
      {#each endpoints as e (e)}<option value={e}></option>{/each}
    </datalist>
    {#if !templated && req.spec.endpoint.trim()}
      <button
        class="tovar"
        title="Use a variable for this URL, so each environment (supplier) can have its own"
        onclick={() => (app.endpointPrompt = req.id)}>{"{{ }}"}</button
      >
    {/if}
    {#if run?.sending}
      <button class="send cancel" onclick={() => app.cancel(req.id)}>Cancel</button>
    {:else}
      <button class="send" onclick={() => app.send(req.id)} title="Send (⌘↵)">Send ▶</button>
    {/if}
  </div>

  {#if resolved}
    <div class="resolved" class:bad={resolved.missing.length > 0}>
      {#if resolved.missing.length}
        ⚠ {resolved.missing.map((m) => `{{${m}}}`).join(", ")} not defined in {scopeLabel}
        <button class="link" onclick={() => (app.varsOpen = true)}>Define…</button>
      {:else}
        → <span class="url" title={resolved.text}>{resolved.text}</span>
        {#if app.activeEnvironment}<span class="env">{app.activeEnvironment.name}</span>{/if}
      {/if}
    </div>
  {/if}

  <div class="subtabs">
    <button class:on={tab === "body"} onclick={() => (tab = "body")}>Body</button>
    <button class:on={tab === "auth"} onclick={() => (tab = "auth")}>
      Auth{#if req.spec.auth.kind !== "none"}<span class="pip"></span>{/if}
    </button>
    <button class:on={tab === "headers"} onclick={() => (tab = "headers")}>
      Headers{#if headerCount}<span class="n">{headerCount}</span>{/if}
    </button>
    <button class:on={tab === "assertions"} onclick={() => (tab = "assertions")}>
      Assertions{#if req.assertions.length}<span class="n">{req.assertions.length}</span>{/if}
    </button>
    <span class="spacer"></span>
    {#if tab === "body"}
      {#if note}<span class="note">{note}</span>{/if}
      <button class="tool" onclick={format} title="Pretty-print the XML">Format</button>
      <button class="tool" onclick={resetToSample} title="Replace the body with the generated sample">Reset to sample</button>
    {/if}
  </div>

  {#if tab === "body" && literalSecrets.length}
    <div class="banner">
      <span>🔒 <code>&lt;{literalSecrets[0].tag}&gt;</code> contains a literal value that would be saved in the project.</span>
      <button class="primary small" onclick={moveToSecret}>Move to secret variable</button>
    </div>
  {/if}

  <div class="content">
    <div class="fill" class:hidden={tab !== "body"}>
      <XmlEditor
        value={req.spec.body}
        modelKey={req.id}
        onChange={(v) => {
          req.spec.body = v;
          app.markDirty();
        }}
        onSend={() => app.send(req.id)}
        vars={app.effectiveVars}
      />
    </div>
    {#if tab === "auth"}
      <AuthEditor {req} />
    {:else if tab === "headers"}
      <HeadersEditor {req} />
    {:else if tab === "assertions"}
      <AssertionsEditor {req} />
    {/if}
  </div>
</section>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  .urlbar {
    display: flex;
    gap: 8px;
    padding: 8px 10px;
    align-items: center;
  }
  .endpoint {
    flex: 1;
    min-width: 80px;
    font-family: var(--mono);
    font-size: 12.5px;
  }
  .tovar {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--accent);
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    padding: 5px 8px;
  }
  .tovar:hover {
    background: var(--hover);
  }
  .resolved {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: -4px 10px 6px 96px;
    font-size: 12px;
    color: var(--muted);
    min-width: 0;
  }
  .resolved .url {
    font-family: var(--mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .resolved .env {
    font-size: 10px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    padding: 0 5px;
    white-space: nowrap;
  }
  .resolved.bad {
    color: var(--warn);
  }
  .link {
    color: var(--accent);
    font-size: 12px;
  }
  .send {
    background: var(--accent);
    color: white;
    padding: 5px 16px;
    border-radius: 6px;
    font-weight: 600;
  }
  .send:hover {
    filter: brightness(1.1);
  }
  .send.cancel {
    background: var(--danger);
  }
  .subtabs {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 8px;
    border-bottom: 1px solid var(--border);
  }
  .subtabs > button:not(.tool) {
    padding: 6px 10px;
    color: var(--muted);
    border-bottom: 2px solid transparent;
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .subtabs > button.on {
    color: var(--fg);
    border-bottom-color: var(--accent);
  }
  .n {
    font-size: 10px;
    background: var(--hover);
    border-radius: 8px;
    padding: 0 5px;
  }
  .pip {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
  }
  .spacer {
    flex: 1;
  }
  .tool {
    font-size: 12px;
    color: var(--muted);
    padding: 3px 8px;
    border-radius: 5px;
  }
  .tool:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .note {
    font-size: 12px;
    color: var(--danger);
    margin-right: 6px;
  }
  .content {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: auto;
  }
  .fill {
    flex: 1;
    min-height: 0;
  }
  .hidden {
    display: none;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: 12px;
    justify-content: space-between;
    padding: 6px 10px 6px 12px;
    background: color-mix(in srgb, var(--warn) 13%, transparent);
    border-bottom: 1px solid var(--border);
    font-size: 12.5px;
  }
  .primary.small {
    padding: 3px 10px;
    font-size: 12px;
    white-space: nowrap;
  }
</style>
