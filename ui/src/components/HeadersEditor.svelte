<script lang="ts">
  import { app } from "../lib/state.svelte";
  import type { SavedRequest } from "../lib/types";

  let { req }: { req: SavedRequest } = $props();
  const dirty = () => app.markDirty();
</script>

<div class="wrap">
  <label class="row">
    <span>SOAPAction</span>
    <input
      value={req.spec.soapAction ?? ""}
      spellcheck="false"
      oninput={(e) => {
        req.spec.soapAction = e.currentTarget.value || null;
        dirty();
      }}
    />
  </label>
  <p class="hint">
    {req.spec.soapVersion === "1.1"
      ? "Sent as the SOAPAction HTTP header with Content-Type text/xml."
      : "Sent as the action parameter of Content-Type application/soap+xml."}
  </p>

  <h4>HTTP headers</h4>
  <table>
    <tbody>
      {#each req.spec.headers as h, i (i)}
        <tr>
          <td class="chk"><input type="checkbox" bind:checked={h.enabled} onchange={dirty} title="Enabled" /></td>
          <td><input placeholder="Name" bind:value={h.name} oninput={dirty} spellcheck="false" /></td>
          <td><input placeholder="Value" bind:value={h.value} oninput={dirty} spellcheck="false" /></td>
          <td class="chk">
            <button
              class="del"
              title="Remove"
              onclick={() => {
                req.spec.headers.splice(i, 1);
                dirty();
              }}>×</button
            >
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
  <button
    class="add"
    onclick={() => {
      req.spec.headers.push({ name: "", value: "", enabled: true });
      dirty();
    }}>+ Add header</button
  >
  <p class="hint">Custom headers override generated ones with the same name (e.g. Content-Type).</p>

  <h4>Connection</h4>
  <label class="row">
    <span>Timeout (seconds)</span>
    <input type="number" min="1" bind:value={req.spec.timeoutSecs} oninput={dirty} style="max-width: 100px" />
  </label>
  <label class="check">
    <input type="checkbox" bind:checked={req.spec.acceptInvalidCerts} onchange={dirty} />
    Accept invalid/self-signed TLS certificates
  </label>
</div>

<style>
  .wrap {
    padding: 14px 16px;
    overflow: auto;
    max-width: 760px;
  }
  .row {
    display: grid;
    grid-template-columns: 110px 1fr;
    align-items: center;
    gap: 10px;
  }
  .row span {
    color: var(--muted);
  }
  h4 {
    margin: 18px 0 8px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  td {
    padding: 2px 3px;
  }
  td input:not([type="checkbox"]) {
    width: 100%;
  }
  .chk {
    width: 24px;
    text-align: center;
  }
  .del {
    color: var(--muted);
    padding: 0 6px;
  }
  .del:hover {
    color: var(--danger);
  }
  .add {
    margin-top: 6px;
    color: var(--accent);
  }
  .hint {
    color: var(--muted);
    font-size: 12px;
    margin: 6px 0 0;
  }
  .check {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-top: 10px;
  }
</style>
