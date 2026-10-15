import { invoke } from '@tauri-apps/api/core';
import { clearS3NavigationCache } from '#lib/state/navigation-cache.svelte.js';

export interface AwsSharedProfile {
  name: string;
  region: string | null;
  sso: boolean;
  login_profile: string | null;
  login_blocked_reason: string | null;
  has_custom_endpoint: boolean;
}
export interface AwsSharedProfiles { profiles: AwsSharedProfile[]; default_profile: string | null }
export interface AwsSsoLoginResult { login_profile: string; cli_version: string }

export async function awsSharedProfiles(): Promise<AwsSharedProfiles> { return invoke('aws_shared_profiles'); }
export async function awsSsoLogin(opId: string, profile: string, loginProfile: string, approved: boolean): Promise<AwsSsoLoginResult> {
  if (!approved) throw new Error('Explicit approval is required before AWS CLI sign-in.');
  clearS3NavigationCache();
  try { return await invoke('aws_sso_login', { opId, profile, loginProfile, approved }); }
  finally { clearS3NavigationCache(); }
}
export async function awsSsoCancel(opId: string): Promise<boolean> { return invoke('aws_sso_cancel', { opId }); }
