import { invoke } from '@tauri-apps/api/core';

export async function keychainSet(profileId: string, secret: string): Promise<void> {
  await invoke('keychain_set', { profileId, secret });
}

export async function keychainGet(profileId: string): Promise<string | null> {
  return await invoke<string | null>('keychain_get', { profileId });
}

export async function keychainDelete(profileId: string): Promise<void> {
  await invoke('keychain_delete', { profileId });
}
