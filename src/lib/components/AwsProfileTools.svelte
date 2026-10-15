<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { awsSharedProfiles, awsSsoLogin, awsSsoCancel, type AwsSharedProfiles } from '#lib/services/aws.js';
  import ErrorNotice from './ErrorNotice.svelte';

  let { profile, allowLogin = true, onUseProfile, onUseRegion }: {
    profile: string;
    allowLogin?: boolean;
    onUseProfile?: (name: string) => void;
    onUseRegion?: (region: string) => void;
  } = $props();
  let report = $state.raw<AwsSharedProfiles | null>(null);
  let loading = $state(false);
  let running = $state(false);
  let approved = $state(false);
  let loadFailure = $state<unknown>(null);
  let loginFailure = $state<unknown>(null);
  let status = $state('');
  let active = true;
  let opId = '';
  let generation = 0;
  let previousProfile = untrack(() => profile);
  const selected = $derived(report?.profiles.find(entry => entry.name === profile));

  $effect(() => {
    if (previousProfile !== profile) {
      previousProfile = profile; generation++; approved = false; status = ''; loginFailure = null;
      if (running && opId) void awsSsoCancel(opId).catch(() => {});
    }
    if (!allowLogin) approved = false;
  });
  onMount(() => {
    void refresh();
    return () => { active = false; generation++; if (opId) void awsSsoCancel(opId).catch(() => {}); };
  });

  async function refresh() {
    if (loading || running) return;
    loading = true; approved = false; loadFailure = null;
    try { const result = await awsSharedProfiles(); if (active) report = result; }
    catch (error) { if (active) { report = null; loadFailure = error; } }
    finally { if (active) loading = false; }
  }

  async function signIn() {
    if (!approved || !allowLogin || !selected?.login_profile || running) return;
    const target = profile;
    const loginProfile = selected.login_profile;
    const current = ++generation;
    const id = 'aws-sso-' + crypto.randomUUID();
    opId = id; running = true; status = ''; loginFailure = null;
    try {
      const result = await awsSsoLogin(id, target, loginProfile, approved);
      if (active && generation === current && profile === target) status = `AWS CLI v${result.cli_version} completed sign-in for ${result.login_profile}. Reconnect explicitly to reload SDK credentials. No bucket access was tested.`;
    } catch (error) { if (active && generation === current && profile === target) loginFailure = error; }
    finally { if (opId === id) { opId = ''; if (active) { running = false; approved = false; } } }
  }

  async function cancel() {
    if (!opId) return;
    try { await awsSsoCancel(opId); if (active) status = 'Cancellation requested. Browser tabs or AWS-managed cached state may remain.'; }
    catch (error) { if (active) loginFailure = error; }
  }
</script>

<div class="aws-profile-tools" data-aws-profile-tools>
  <div class="actions">
    <strong>AWS shared profiles / IAM Identity Center</strong>
    <button type="button" onclick={refresh} disabled={loading || running}>{loading ? 'Reading local profiles…' : 'Refresh profiles'}</button>
  </div>
  <p>Discovery reads local AWS config/credentials only. It does not request credentials, run credential_process, or read SSO token caches. Session validity is checked when you connect.</p>
  {#if loadFailure}<ErrorNotice error={loadFailure} context="AWS profile discovery" />{/if}
  {#if report}
    {#if onUseProfile}
      <label>Choose a discovered profile
        <select value={report.profiles.some(entry => entry.name === profile) ? profile : ''} disabled={running} onchange={(event) => { if (event.currentTarget.value) onUseProfile?.(event.currentTarget.value); }}>
          <option value="">Select explicitly — keep current form until selected</option>
          {#each report.profiles as entry (entry.name)}<option value={entry.name}>{entry.name}{entry.sso ? ' — SSO' : ' — shared profile'}</option>{/each}
        </select>
      </label>
      <p>Choosing a profile switches this form away from manual keys. Other form fields, including role, proxy and encryption settings, are unchanged. No endpoints or secret values are imported.</p>
    {/if}
    {#if report.default_profile}<p>Default shared-profile name: {report.default_profile}. Environment credentials may take precedence in the default chain.</p>{/if}
    {#if report.profiles.length === 0}<p>No shared profiles found. Configure AWS CLI v2 externally, or keep using manual credentials.</p>{/if}
    {#if selected?.region && onUseRegion}<button type="button" disabled={running} onclick={() => onUseRegion?.(selected!.region!)}>Use this profile's S3 region: {selected.region}</button>{/if}
    {#if selected?.sso}
      {#if selected.login_blocked_reason}<p>{selected.login_blocked_reason}</p>{/if}
      {#if selected.login_profile}
        <p>Selected profile: <strong>{profile}</strong>. Sign-in target: <strong>{selected.login_profile}</strong>{selected.login_profile !== profile ? ' (SSO source of the role chain)' : ''}.</p>
        <p>This explicitly runs AWS CLI v2 <code>sso login</code> without a shell and may open your browser. AWS CLI manages its own cached tokens; Furman does not copy/store them. CLI proxy/environment settings are separate from Furman's connection proxy.</p>
        {#if !allowLogin}<p>Manual credentials take precedence. Choose the discovered profile or clear manual keys before enabling SSO sign-in.</p>{/if}
        <label><input type="checkbox" bind:checked={approved} disabled={running || !allowLogin} /> I approve running AWS CLI sign-in for this profile, opening a browser and updating the AWS-managed session cache.</label>
        <div class="actions"><button type="button" onclick={signIn} disabled={!approved || !allowLogin || running}>{running ? 'Waiting for AWS CLI / browser…' : 'Sign in with AWS SSO'}</button>
          {#if running}<button type="button" onclick={cancel}>Cancel sign-in</button>{/if}
        </div>
        <p>Sign-in is limited to five minutes. It does not grant S3 permissions, reconnect panels or retry transfers. Existing connections may need an explicit reconnect.</p>
      {/if}
    {:else if profile}<p>{selected ? 'This shared profile does not declare SSO; no SSO command will be run.' : 'This name is not in the discovered list. Refresh profiles or configure it externally; manual profile input is still available.'}</p>
    {:else}<p>Select an explicit SSO profile to enable sign-in; the default chain is never signed in automatically.</p>{/if}
  {/if}
  {#if loginFailure}<ErrorNotice error={loginFailure} context="AWS SSO sign-in" />{/if}
  {#if status}<p role="status">{status}</p>{/if}
</div>

<style>
  .aws-profile-tools { margin: 8px 0 16px; padding: 10px; border: 1px solid var(--border-subtle); border-radius: var(--radius-sm); font-size: 12px; }
  .actions { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; } p { color: var(--text-secondary); overflow-wrap: anywhere; margin: 8px 0; } label { display: block; margin: 8px 0; } select { display: block; max-width: 100%; margin-top: 6px; } button, select { font: inherit; }
</style>
