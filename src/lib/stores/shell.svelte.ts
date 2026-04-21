class ShellStore {
  shellMap = $state(new Map());
  shellOpenMap = $state<Record<string, boolean>>({});
  activeShellEntry = $state<any>(null);
  shellWidthMap = $state<Record<string, number>>({});
  isDraggingDivider = $state(false);
  focusedPanel = $state<'claude' | 'shell'>('claude');

  getShellWidth(profileId: string): number {
    return this.shellWidthMap[profileId] ?? 50;
  }
}

export const shellStore = new ShellStore();
