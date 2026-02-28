<script lang="ts">
  import { appState } from '$lib/state/app.svelte';
  import { onMount } from 'svelte';
  import logoLight from '$lib/assets/furman-logo-light.svg';
  import logoDark from '$lib/assets/furman-logo-dark.svg';

  const logo = $derived(appState.theme === 'dark' ? logoDark : logoLight);

  let version = $state('');
  onMount(async () => {
    try {
      const { getVersion } = await import('@tauri-apps/api/app');
      version = await getVersion();
    } catch {
      version = '';
    }
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="menu-backdrop"
  role="presentation"
  onclick={() => { appState.menuActive = false; }}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="menu-dropdown no-select"
    role="menu"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => { if (e.key === 'Escape') appState.menuActive = false; }}
  >
    <button
      class="menu-row"
      role="menuitem"
      onclick={() => { appState.menuActive = false; appState.showPreferences(); }}
    >
      <span class="check"></span>
      Preferences...
    </button>

    <button
      class="menu-row"
      role="menuitem"
      onclick={() => { appState.menuActive = false; appState.modal = 'shortcuts'; }}
    >
      <span class="check"></span>
      Shortcuts...
      <span class="menu-shortcut">{'\u2318'}/</span>
    </button>

    <div class="menu-divider"></div>

    <div class="about-section">
      <img class="about-logo" src={logo} alt="Furman" />
      <div class="about-name">Furman</div>
      <div class="about-version">{version ? `v${version}` : ''}</div>
      <div class="about-desc">Dual-pane file manager</div>
    </div>
  </div>
</div>

<style>
  .menu-backdrop {
    position: fixed;
    inset: 0;
    z-index: 90;
  }

  .menu-dropdown {
    position: fixed;
    top: 40px;
    right: 12px;
    background: var(--dialog-bg);
    border: 1px solid var(--dialog-border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-dialog);
    min-width: 200px;
    padding: 6px 0;
    z-index: 91;
  }

  .menu-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 16px;
    font-size: 13px;
    color: var(--text-primary);
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    font-family: inherit;
    transition: background var(--transition-fast);
  }

  .menu-row:hover {
    background: var(--bg-hover);
  }

  .check {
    width: 14px;
    text-align: center;
    font-size: 12px;
    flex-shrink: 0;
  }

  .menu-shortcut {
    margin-left: auto;
    font-size: 11px;
    color: var(--text-secondary);
    opacity: 0.6;
  }

  .menu-divider {
    height: 1px;
    background: var(--border-subtle);
    margin: 4px 0;
  }

  .about-section {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 12px 16px 10px;
    gap: 2px;
  }

  .about-logo {
    width: 56px;
    height: 56px;
    border-radius: 12px;
    margin-bottom: 4px;
  }

  .about-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .about-version {
    font-size: 11px;
    color: var(--text-secondary);
    opacity: 0.7;
  }

  .about-desc {
    font-size: 11px;
    color: var(--text-secondary);
    opacity: 0.5;
  }
</style>
