<script lang="ts">
  import { appState } from '#lib/state/app.svelte.js';
  import ErrorNotice from './ErrorNotice.svelte';
</script>

{#if appState.configSaveError}
  <div class="config-save-notice" role="alert">
    <ErrorNotice error={appState.configSaveError} context="Configuration could not be saved; changes may not survive restart" />
    <button type="button" onclick={() => { appState.configSaveError = ''; }}>Dismiss message</button>
  </div>
{/if}

<style>
  .config-save-notice { position: fixed; right: 12px; top: 12px; width: min(540px, 90vw); max-height: 40vh; overflow: auto; z-index: 2000; padding: 12px; background: var(--dialog-bg); color: var(--text-primary); border: 1px solid var(--color-error, #ef4444); border-radius: var(--radius-sm); box-shadow: var(--shadow-dialog); }
  button { margin-top: 8px; font: inherit; font-size: 12px; }
</style>
