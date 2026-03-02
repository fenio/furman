import type { TrashInfo } from '$lib/services/tauri';

export interface Operation {
  id: string;
  type: 'delete' | 'rename' | 'move';
  timestamp: number;
  backend: string;
  trashItems?: TrashInfo[];
  originalPath?: string;
  newPath?: string;
  originalName?: string;
  newName?: string;
  sourcePaths?: string[];
  destination?: string;
  undone: boolean;
}

const MAX_HISTORY = 20;

class OperationsState {
  history: Operation[] = $state([]);
  toastVisible = $state(false);
  private toastTimer: ReturnType<typeof setTimeout> | null = null;

  get lastUndoable(): Operation | undefined {
    return this.history.find((op) => !op.undone && op.backend === 'local');
  }

  push(op: Operation) {
    this.history.unshift(op);
    if (this.history.length > MAX_HISTORY) {
      this.history.length = MAX_HISTORY;
    }
    this.showToast();
  }

  undo(): Operation | undefined {
    const op = this.lastUndoable;
    if (op) {
      op.undone = true;
      this.dismissToast();
    }
    return op;
  }

  showToast() {
    this.toastVisible = true;
    if (this.toastTimer) clearTimeout(this.toastTimer);
    this.toastTimer = setTimeout(() => {
      this.toastVisible = false;
      this.toastTimer = null;
    }, 8000);
  }

  dismissToast() {
    this.toastVisible = false;
    if (this.toastTimer) {
      clearTimeout(this.toastTimer);
      this.toastTimer = null;
    }
  }
}

export const operationsState = new OperationsState();
