<script lang="ts">
  import { onMount } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { WebLinksAddon } from '@xterm/addon-web-links';
  import '@xterm/xterm/css/xterm.css';
  import { terminalSpawn, terminalWrite, terminalResize, terminalClose } from '$lib/services/tauri.ts';
  import { listen } from '@tauri-apps/api/event';
  import { appState } from '$lib/state/app.svelte.ts';
  import type { TerminalOutput, TerminalExit } from '$lib/types';

  const darkTheme = {
    background: '#1e1e1e',
    foreground: '#e0e0e0',
    cursor: '#e0e0e0',
    selectionBackground: '#264f78',
    black: '#1e1e1e',
    red: '#f44747',
    green: '#6a9955',
    yellow: '#dcdcaa',
    blue: '#569cd6',
    magenta: '#c586c0',
    cyan: '#4fc3f7',
    white: '#e0e0e0',
    brightBlack: '#808080',
    brightRed: '#f44747',
    brightGreen: '#6a9955',
    brightYellow: '#dcdcaa',
    brightBlue: '#569cd6',
    brightMagenta: '#c586c0',
    brightCyan: '#4fc3f7',
    brightWhite: '#ffffff',
  };

  const lightTheme = {
    background: '#f5f5f7',
    foreground: '#1d1d1f',
    cursor: '#1d1d1f',
    selectionBackground: '#b3d7ff',
    black: '#1d1d1f',
    red: '#d32f2f',
    green: '#2e7d32',
    yellow: '#b8860b',
    blue: '#0066cc',
    magenta: '#9c27b0',
    cyan: '#00838f',
    white: '#f5f5f7',
    brightBlack: '#6e6e73',
    brightRed: '#d32f2f',
    brightGreen: '#2e7d32',
    brightYellow: '#b8860b',
    brightBlue: '#0066cc',
    brightMagenta: '#9c27b0',
    brightCyan: '#00838f',
    brightWhite: '#ffffff',
  };

  interface Props {
    terminalId: string;
    cwd: string;
    onExit?: (id: string) => void;
    onCwdChange?: (cwd: string) => void;
  }

  let { terminalId, cwd, onExit, onCwdChange }: Props = $props();

  let containerEl: HTMLDivElement;
  let terminal: Terminal;
  let fitAddon: FitAddon;
  let unlistenOutput: (() => void) | null = null;
  let unlistenExit: (() => void) | null = null;
  let resizeObserver: ResizeObserver | null = null;

  onMount(() => {
    terminal = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: "'SF Mono', 'Menlo', 'Monaco', 'Courier New', monospace",
      theme: appState.theme === 'light' ? lightTheme : darkTheme,
    });

    fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.loadAddon(new WebLinksAddon());

    terminal.open(containerEl);

    // Register OSC 7 handler for cwd tracking
    terminal.parser.registerOscHandler(7, (data: string) => {
      // data is "file://hostname/path" or "file:///path"
      const match = data.match(/^file:\/\/[^/]*(\/.*)/);
      if (match) {
        const path = decodeURIComponent(match[1]);
        onCwdChange?.(path);
      }
      return true;
    });

    // Initial fit + focus
    requestAnimationFrame(() => {
      fitAddon.fit();
      terminal.focus();
      init();
    });

    // ResizeObserver for automatic re-fitting
    resizeObserver = new ResizeObserver(() => {
      fitAddon.fit();
      if (terminal.cols && terminal.rows) {
        terminalResize(terminalId, terminal.cols, terminal.rows).catch(() => {});
      }
    });
    resizeObserver.observe(containerEl);

    return () => {
      // Cleanup
      resizeObserver?.disconnect();
      unlistenOutput?.();
      unlistenExit?.();
      terminalClose(terminalId).catch(() => {});
      terminal.dispose();
    };
  });

  $effect(() => {
    if (terminal) {
      terminal.options.theme = appState.theme === 'light' ? lightTheme : darkTheme;
    }
  });

  async function init() {
    // Listen for PTY output
    unlistenOutput = await listen<TerminalOutput>('terminal-output', (event) => {
      if (event.payload.id === terminalId) {
        terminal.write(event.payload.data);
      }
    });

    // Listen for PTY exit
    unlistenExit = await listen<TerminalExit>('terminal-exit', (event) => {
      if (event.payload.id === terminalId) {
        terminal.write('\r\n[Process exited]\r\n');
        onExit?.(terminalId);
      }
    });

    // Wire terminal input to PTY
    terminal.onData((data) => {
      terminalWrite(terminalId, data).catch(() => {});
    });

    // Spawn the PTY
    try {
      await terminalSpawn(terminalId, cwd);
      // Send initial resize
      if (terminal.cols && terminal.rows) {
        await terminalResize(terminalId, terminal.cols, terminal.rows);
      }
    } catch (err) {
      terminal.write(`\r\nFailed to spawn terminal: ${err}\r\n`);
    }
  }
</script>

<div class="xterm-container" bind:this={containerEl}></div>

<style>
  .xterm-container {
    width: 100%;
    height: 100%;
    overflow: hidden;
  }

  .xterm-container :global(.xterm) {
    height: 100%;
    padding: 4px;
  }

  .xterm-container :global(.xterm-viewport) {
    overflow-y: auto !important;
  }
</style>
