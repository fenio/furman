import { appState } from '$lib/state/app.svelte';

export interface Workspace {
  name: string;
  leftPath: string;
  rightPath: string;
  activePanel: 'left' | 'right';
}

class WorkspacesState {
  workspaces = $state<Workspace[]>([]);

  load(workspaces?: Workspace[]) {
    if (workspaces) {
      this.workspaces = workspaces;
    }
  }

  save(workspace: Workspace) {
    const idx = this.workspaces.findIndex((w) => w.name === workspace.name);
    if (idx >= 0) {
      this.workspaces[idx] = workspace;
    } else {
      this.workspaces = [...this.workspaces, workspace];
    }
    this.persist();
  }

  remove(name: string) {
    this.workspaces = this.workspaces.filter((w) => w.name !== name);
    this.persist();
  }

  private persist() {
    appState.persistConfig();
  }
}

export const workspacesState = new WorkspacesState();
