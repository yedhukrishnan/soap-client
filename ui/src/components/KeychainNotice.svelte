<script lang="ts">
  import { app, type KeychainNotice } from "../lib/state.svelte";

  let { notice }: { notice: KeychainNotice } = $props();

  const mac = $derived(app.secretBackend?.name.includes("macOS") ?? false);
  const SERVICE = "dev.soapclient.app";

  function focus(n: HTMLElement) {
    n.focus();
  }
</script>

<div class="backdrop" role="presentation">
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="kc-title"
    tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && notice.resolve(false)}
  >
    <h3 id="kc-title">🔑 How secrets are stored</h3>

    <p>
      Secret variables (passwords, tokens, API keys) are saved in
      {mac ? "your Mac's Keychain" : "your system keyring (GNOME Keyring / KWallet)"}, never in project files, and
      are only sent to your requests' endpoints.
    </p>

    {#if mac}
      <h4>If macOS asks for permission</h4>
      <p>
        You may see <i>“SOAP Client wants to use your confidential information stored in “{SERVICE}”…”</i>.
        Choose <b>Always Allow</b> so it doesn't ask again. With <b>Deny</b>, requests that use secrets won't be sent.
      </p>
    {:else}
      <h4>If your keyring is locked</h4>
      <p>Your desktop will ask you to unlock it with your login password.</p>
    {/if}

    <h4>What the app can access</h4>
    <ul>
      <li>Only its own entry, <code>{SERVICE}</code>. It can't read your other passwords.</li>
      <li>If a prompt ever names anything else, choose <b>Deny</b>.</li>
      <li>
        To view or delete it, search for <code>{SERVICE}</code> in
        {mac ? "Keychain Access" : "your keyring manager (e.g. Seahorse)"}.
      </li>
    </ul>

    <div class="actions">
      {#if notice.mode === "consent"}
        <button class="secondary" onclick={() => notice.resolve(false)}>Cancel</button>
        <button class="primary" use:focus onclick={() => notice.resolve(true)}>Continue</button>
      {:else}
        <button class="primary" use:focus onclick={() => notice.resolve(true)}>OK</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 80;
  }
  .dialog {
    width: min(480px, 92vw);
    max-height: 90vh;
    overflow: auto;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow);
    padding: 20px 22px;
    line-height: 1.5;
    user-select: text;
    -webkit-user-select: text;
  }
  h3 {
    margin: 0 0 10px;
  }
  h4 {
    margin: 14px 0 2px;
    font-size: 13px;
    font-weight: 600;
  }
  p {
    margin: 6px 0;
  }
  ul {
    margin: 4px 0;
    padding-left: 20px;
  }
  li {
    margin: 3px 0;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 18px;
  }
</style>
