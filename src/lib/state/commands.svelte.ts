export interface Command {
  id: string;
  label: string;
  shortcut?: string;
  category: 'File' | 'Navigation' | 'Panel' | 'Terminal' | 'Connection' | 'S3' | 'Search' | 'Display';
  execute: () => void;
  enabled?: () => boolean;
}

export const commandRegistry: Command[] = $state([]);
