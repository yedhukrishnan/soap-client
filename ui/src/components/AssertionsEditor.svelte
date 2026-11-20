<script lang="ts">
  import { app, uid } from "../lib/state.svelte";
  import type { Check, SavedRequest } from "../lib/types";

  let { req }: { req: SavedRequest } = $props();

  const LABELS: Record<Check["type"], string> = {
    statusCode: "HTTP status is",
    noSoapFault: "No SOAP Fault",
    contains: "Body contains",
    notContains: "Body does not contain",
    xpath: "XPath match",
    maxResponseTime: "Response time ≤",
  };

  const defaults: Record<Check["type"], () => Check> = {
    statusCode: () => ({ type: "statusCode", code: 200 }),
    noSoapFault: () => ({ type: "noSoapFault" }),
    contains: () => ({ type: "contains", text: "", ignoreCase: false }),
    notContains: () => ({ type: "notContains", text: "", ignoreCase: false }),
    xpath: () => ({ type: "xpath", expression: "", expected: "" }),
    maxResponseTime: () => ({ type: "maxResponseTime", ms: 1000 }),
  };

  let addType = $state<Check["type"]>("statusCode");
  const results = $derived(app.runs[req.id]?.results ?? []);
  const resultFor = (id: string) => results.find((r) => r.id === id);

  function changed() {
    app.markDirty();
    app.rerunAssertions(req.id);
  }
</script>

<div class="wrap">
  {#each req.assertions as a, i (a.id)}
    {@const res = resultFor(a.id)}
    <div class="row" class:disabled={!a.enabled}>
      <input type="checkbox" bind:checked={a.enabled} onchange={changed} title="Enabled" />
      <span class="status" class:pass={res?.passed} class:fail={res && !res.passed} title={res?.message ?? "Not run yet"}>
        {res ? (res.passed ? "✔" : "✘") : "○"}
      </span>
      <span class="kind">{LABELS[a.check.type]}</span>
      <div class="params">
        {#if a.check.type === "statusCode"}
          <input type="number" bind:value={a.check.code} oninput={changed} class="num" />
        {:else if a.check.type === "maxResponseTime"}
          <input type="number" bind:value={a.check.ms} oninput={changed} class="num" /> ms
        {:else if a.check.type === "contains" || a.check.type === "notContains"}
          <input placeholder="text" bind:value={a.check.text} oninput={changed} spellcheck="false" />
          <label class="ic"><input type="checkbox" bind:checked={a.check.ignoreCase} onchange={changed} /> ignore case</label>
        {:else if a.check.type === "xpath"}
          <input
            placeholder="e.g. //*[local-name()='AddResult']"
            bind:value={a.check.expression}
            oninput={changed}
            spellcheck="false"
            class="mono"
          />
          <span class="eq">=</span>
          <input placeholder="expected (blank = exists)" bind:value={a.check.expected} oninput={changed} spellcheck="false" />
        {/if}
      </div>
      <button
        class="del"
        title="Remove"
        onclick={() => {
          req.assertions.splice(i, 1);
          changed();
        }}>×</button
      >
      {#if res && !res.passed}<div class="msg">{res.message}</div>{/if}
    </div>
  {:else}
    <p class="empty">No assertions. Add checks that run automatically after each response.</p>
  {/each}

  <div class="add">
    <select bind:value={addType}>
      {#each Object.entries(LABELS) as [k, label] (k)}
        <option value={k}>{label}</option>
      {/each}
    </select>
    <button
      onclick={() => {
        req.assertions.push({ id: uid(), enabled: true, check: defaults[addType]() });
        changed();
      }}>+ Add assertion</button
    >
  </div>
  <p class="hint">
    XPath 1.0: namespace prefixes declared in the response can be used directly (plus <code>soap11</code> /
    <code>soap12</code>), or use <code>local-name()</code>.
  </p>
</div>

<style>
  .wrap {
    padding: 10px 16px;
    overflow: auto;
  }
  .row {
    display: grid;
    grid-template-columns: 20px 20px 170px 1fr 24px;
    align-items: center;
    gap: 6px;
    padding: 4px 0;
    border-bottom: 1px solid var(--border);
  }
  .row.disabled {
    opacity: 0.5;
  }
  .status {
    text-align: center;
    color: var(--muted);
  }
  .pass {
    color: var(--ok);
  }
  .fail {
    color: var(--danger);
  }
  .kind {
    font-weight: 500;
  }
  .params {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .params input:not([type="checkbox"]) {
    flex: 1;
    min-width: 0;
  }
  .params .num {
    flex: 0 0 90px;
  }
  .ic {
    white-space: nowrap;
    color: var(--muted);
    font-size: 12px;
  }
  .eq {
    color: var(--muted);
  }
  .msg {
    grid-column: 3 / 5;
    color: var(--danger);
    font-size: 12px;
  }
  .del {
    color: var(--muted);
  }
  .del:hover {
    color: var(--danger);
  }
  .add {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }
  .add button {
    color: var(--accent);
  }
  .empty,
  .hint {
    color: var(--muted);
    font-size: 12px;
  }
</style>
