import type { TerminalDisplayMode } from '$lib/types';

export interface TerminalInstance {
  id: string;
  cwd: string;
}

let nextCounter = 0;

class TerminalStateClass {
  displayMode = $state<TerminalDisplayMode>('none');
  instances = $state<TerminalInstance[]>([]);
  activeIndex = $state(0);
  inPaneSlot = $state<'left' | 'right'>('right');
  bottomPanelHeight = $state(250);
  quakeHeight = $state(40); // vh%

  get activeInstance(): TerminalInstance | null {
    return this.instances[this.activeIndex] ?? null;
  }

  nextId(): string {
    nextCounter++;
    return `term-${nextCounter}`;
  }

  addInstance(cwd: string) {
    const id = this.nextId();
    this.instances = [...this.instances, { id, cwd }];
    this.activeIndex = this.instances.length - 1;
  }

  removeInstance(id: string) {
    const idx = this.instances.findIndex((t) => t.id === id);
    if (idx === -1) return;
    this.instances = this.instances.filter((t) => t.id !== id);
    if (this.activeIndex >= this.instances.length) {
      this.activeIndex = Math.max(0, this.instances.length - 1);
    }
  }

  toggle(mode: TerminalDisplayMode) {
    if (this.displayMode === mode) {
      this.displayMode = 'none';
    } else {
      this.displayMode = mode;
      // Ensure at least one terminal instance exists
      if (this.instances.length === 0) {
        // cwd will be set by the component from the active panel
        this.addInstance('');
      }
    }
  }
}

export const terminalState = new TerminalStateClass();
