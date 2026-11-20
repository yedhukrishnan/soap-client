<script lang="ts">
  import { app } from "../lib/state.svelte";
</script>

<div class="tabbar">
  {#each app.openTabs as id (id)}
    {@const r = app.request(id)}
    {#if r}
      <div
        class="tab"
        class:active={app.activeId === id}
        role="tab"
        tabindex="0"
        aria-selected={app.activeId === id}
        title="{app.interfaceLabel(r.wsdlId, r.interfaceName)} › {r.operationName} › {r.name}"
        onclick={() => (app.activeId = id)}
        onkeydown={(e) => e.key === "Enter" && (app.activeId = id)}
        onauxclick={(e) => e.button === 1 && app.closeTab(id)}
      >
        {#if app.runs[id]?.sending}<span class="spinner"></span>{/if}
        <span class="label"><b>{r.operationName}</b> · {r.name}</span>
        <button
          class="close"
          title="Close tab"
          onclick={(e) => {
            e.stopPropagation();
            app.closeTab(id);
          }}>×</button
        >
      </div>
    {/if}
  {/each}
  <button class="plus" title="New request for this operation" disabled={!app.active} onclick={() => app.newRequestLikeActive()}
    >+</button
  >
</div>

<style>
  .tabbar {
    display: flex;
    align-items: stretch;
    height: 36px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-side);
    overflow-x: auto;
    overflow-y: hidden;
    flex-shrink: 0;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 6px 0 12px;
    border-right: 1px solid var(--border);
    cursor: default;
    white-space: nowrap;
    color: var(--muted);
    max-width: 260px;
  }
  .tab.active {
    background: var(--bg);
    color: var(--fg);
    box-shadow: inset 0 -2px 0 var(--accent);
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .label b {
    font-weight: 600;
  }
  .close {
    opacity: 0;
    padding: 0 4px;
    border-radius: 4px;
    color: var(--muted);
  }
  .tab:hover .close,
  .tab.active .close {
    opacity: 1;
  }
  .close:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .plus {
    padding: 0 14px;
    color: var(--muted);
    font-size: 16px;
  }
  .plus:hover:not(:disabled) {
    color: var(--fg);
    background: var(--hover);
  }
  .plus:disabled {
    opacity: 0.4;
  }
</style>
