import { theme } from './theme.svelte';

class TerminalStore {
  terminalMap = $state(new Map());
  activeTermEntry = $state<any>(null);
  currentTerminalId = $state<string | null>(null);
  termFontSize = $state(13);
  sessionActivity = $state<Record<string, string | null>>({});

  constructor() {
    if (typeof localStorage !== 'undefined') {
      this.termFontSize = parseInt(localStorage.getItem('clauge-font-size') || '13');
    }
  }

  getTermConfig() {
    return {
      theme: theme.getTermTheme(),
      fontFamily: '"JetBrains Mono", "Fira Code", "Cascadia Code", "SF Mono", "Source Code Pro", "IBM Plex Mono", "Menlo", "Monaco", "Consolas", monospace',
      fontSize: this.termFontSize,
      lineHeight: 1.4,
      cursorBlink: true,
      cursorStyle: 'bar' as const,
      scrollback: 10000,
    };
  }
}

export const terminalStore = new TerminalStore();
