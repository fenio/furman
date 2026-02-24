import type { S3Profile } from '$lib/types';
import { keychainSet, keychainGet, keychainDelete } from '$lib/services/keychain';
import { appState } from '$lib/state/app.svelte';

class S3ProfilesState {
  profiles = $state<S3Profile[]>([]);

  load(profiles?: S3Profile[]) {
    if (profiles) {
      this.profiles = profiles;
    }
  }

  addProfile(profile: S3Profile) {
    this.profiles = [...this.profiles, profile];
    this.persist();
  }

  updateProfile(profile: S3Profile) {
    const idx = this.profiles.findIndex((p) => p.id === profile.id);
    if (idx >= 0) {
      this.profiles[idx] = profile;
      this.profiles = [...this.profiles];
    }
    this.persist();
  }

  removeProfile(id: string) {
    this.profiles = this.profiles.filter((p) => p.id !== id);
    this.persist();
  }

  async saveSecret(profileId: string, secret: string): Promise<void> {
    await keychainSet(profileId, secret);
  }

  async getSecret(profileId: string): Promise<string | null> {
    return await keychainGet(profileId);
  }

  async deleteSecret(profileId: string): Promise<void> {
    await keychainDelete(profileId);
  }

  private persist() {
    appState.persistConfig();
  }
}

export const s3ProfilesState = new S3ProfilesState();
