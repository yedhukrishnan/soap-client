<script lang="ts">
  import { app } from "../lib/state.svelte";
  import type { SavedRequest } from "../lib/types";
  import XmlEditor from "./XmlEditor.svelte";

  let { req }: { req: SavedRequest } = $props();
  let tab = $state<"pretty" | "raw" | "headers" | "assertions" | "request">("pretty");

  const run = $derived(app.runs[req.id]);
  const resp = $derived(run?.response);
  const results = $derived(run?.results ?? []);
  const passed = $derived(results.filter((r) => r.passed).length);
  const statusClass = $derived(!resp ? "" : resp.status < 300 ? "ok" : resp.status < 500 ? "warn" : "bad");

  function size(n: number) {
    return n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(1)} MB`;
  }

  const sentText = $derived(
    resp
      ? `POST ${resp.request.url}\n${resp.request.headers.map(([k, v]) => `${k}: ${v}`).join("\n")}\n\n${resp.request.body}`
      : "",
  );
</script>

<section class="pane">
  <div class="status">
    {#if run?.sending}
      <span class="spinner"></span> Sending…
    {:else if run?.error}
      <span class="dot bad"></span><span class="err">{run.error}</span>
    {:else if resp}
      <span class="dot {statusClass}"></span>
      <b>{resp.status} {resp.statusText}</b>
      <span class="meta">{resp.elapsedMs} ms</span>
      <span class="meta">{size(resp.sizeBytes)}</span>
      {#if results.length}
        <button class="asum" class:allok={passed === results.length} onclick={() => (tab = "assertions")}>
          {passed === results.length ? "✔" : "✘"}
          {passed}/{results.length} assertions
        </button>
      {/if}
    {:else}
      <span class="meta">No response yet — press <b>Send</b> or ⌘↵</span>
    {/if}
  </div>

  {#if resp}
    <div class="subtabs">
      {#each [["pretty", "Pretty"], ["raw", "Raw"], ["headers", "Headers"], ["assertions", "Assertions"], ["request", "Request"]] as [k, label] (k)}
        <button class:on={tab === k} onclick={() => (tab = k as typeof tab)}>{label}</button>
      {/each}
    </div>
    <div class="content">
      {#if tab === "pretty"}
        <XmlEditor value={resp.bodyPretty ?? resp.body} readOnly wordWrap={!resp.bodyPretty} />
      {:else if tab === "raw"}
        <XmlEditor value={resp.body} readOnly wordWrap />
      {:else if tab === "request"}
        <XmlEditor value={sentText} readOnly wordWrap />
      {:else if tab === "headers"}
        <table class="kv">
          <tbody>
            {#each resp.headers as [k, v], i (i)}
              <tr><td>{k}</td><td>{v}</td></tr>
            {/each}
          </tbody>
        </table>
      {:else}
        <div class="results">
          {#each results as r (r.id)}
            {@const a = req.assertions.find((x) => x.id === r.id)}
            <div class="res" class:pass={r.passed}>
              <span class="mark">{r.passed ? "✔" : "✘"}</span>
              <span class="what">{a?.check.type ?? "?"}</span>
              <span class="msg">{r.message}</span>
            </div>
          {:else}
            <p class="meta">No assertions configured for this request.</p>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</section>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 7px 12px;
    border-bottom: 1px solid var(--border);
    min-height: 34px;
    flex-shrink: 0;
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .dot.ok {
    background: var(--ok);
  }
  .dot.warn {
    background: var(--warn);
  }
  .dot.bad {
    background: var(--danger);
  }
  .err {
    color: var(--danger);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    color: var(--muted);
  }
  .asum {
    margin-left: auto;
    color: var(--danger);
    font-weight: 500;
  }
  .asum.allok {
    color: var(--ok);
  }
  .subtabs {
    display: flex;
    gap: 2px;
    padding: 0 8px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .subtabs button {
    padding: 5px 10px;
    color: var(--muted);
    border-bottom: 2px solid transparent;
  }
  .subtabs button.on {
    color: var(--fg);
    border-bottom-color: var(--accent);
  }
  .content {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .kv {
    border-collapse: collapse;
    font-family: var(--mono);
    font-size: 12px;
    margin: 8px 12px;
  }
  .kv td {
    padding: 3px 12px 3px 0;
    vertical-align: top;
  }
  .kv td:first-child {
    color: var(--muted);
    white-space: nowrap;
  }
  .results {
    padding: 8px 12px;
  }
  .res {
    display: grid;
    grid-template-columns: 20px 140px 1fr;
    gap: 8px;
    padding: 4px 0;
    border-bottom: 1px solid var(--border);
    color: var(--danger);
  }
  .res.pass {
    color: var(--ok);
  }
  .res .what {
    color: var(--fg);
    font-weight: 500;
  }
  .res .msg {
    color: var(--muted);
  }
</style>
