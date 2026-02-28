import type { ConnectionProfile, S3Profile, SftpProfile } from '$lib/types';
import { keychainSet, keychainGet, keychainDelete } from '$lib/services/keychain';
import { appState } from '$lib/state/app.svelte';

class ConnectionsState {
  profiles = $state<ConnectionProfile[]>([]);

  get s3Profiles(): S3Profile[] {
    return this.profiles.filter((p): p is S3Profile => p.type === 's3');
  }

  get sftpProfiles(): SftpProfile[] {
    return this.profiles.filter((p): p is SftpProfile => p.type === 'sftp');
  }

  load(profiles?: ConnectionProfile[]) {
    if (profiles) {
      this.profiles = profiles;
    }
  }

  addProfile(profile: ConnectionProfile) {
    this.profiles = [...this.profiles, profile];
    this.persist();
  }

  updateProfile(profile: ConnectionProfile) {
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

export const connectionsState = new ConnectionsState();
