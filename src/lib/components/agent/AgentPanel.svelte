<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { get } from 'svelte/store';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import '@xterm/xterm/css/xterm.css';
  import { Channel } from '@tauri-apps/api/core';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import {
    activeAgentSession,
    agentTerminalMap,
    agentShellMap,
    agentTerminalIds,
    agentShellIds,
    agentShellOpen,
    agentSessionActivity,
    agentSessions,
    agentNotifyEnabled,
    agentSoundEnabled,
    agentDockBounceEnabled,
  } from '$lib/stores/agent';
  import { getSetting } from '$lib/commands/settings';
  import {
    agentSpawnTerminal,
    agentSpawnShell,
    agentWriteToTerminal,
    agentResizeTerminal,
    agentUpdateSessionId,
    agentUpdateLastUsed,
    agentDiscoverSessions,
    agentIsGitRepo,
    agentCreateWorktree,
    agentUpdateWorktree,
    agentListSessions,
    agentGetSessionContexts,
    agentInjectContexts,
    agentKillTerminal,
    agentRemoveWorktree,
    agentDeleteSession,
  } from '$lib/commands/agent';
  import { refreshAgentGitStatus, refreshAgentContextUsage, loadAgentSessions } from '$lib/stores/agent';
  import { getTerminalTheme } from '$lib/utils/theme';
  import { appearance } from '$lib/stores/settings';
  import { getPurposePrompt } from '$lib/prompts/agent';

  let terminalEl: HTMLDivElement;
  let shellEl: HTMLDivElement;
  let wrapperEl: HTMLDivElement;

  // Active terminal entry refs
  let activeTermEntry: { term: Terminal; fitAddon: FitAddon; container: HTMLDivElement; terminalId: string | null; _exitBuffer?: string } | null = null;
  let activeShellEntry: { term: Terminal; fitAddon: FitAddon; container: HTMLDivElement; terminalId: string | null } | null = null;

  // Divider drag state
  let dragging = false;
  let mainHeight = 65; // percentage
  let fitTimer: ReturnType<typeof setTimeout> | null = null;

  // Track current session to detect changes
  let currentSessionId: string | null = null;

  // Context usage polling interval
  let contextUsageInterval: ReturnType<typeof setInterval> | null = null;

  // Suppress auto-switch on exit (set by reset/close actions)
  let _suppressExit = false;

  // --- Notification system for action-required prompts ---
  let notifyOutputBuffer = '';
  let notifyLastTime = 0;
  let notifyBufferTimer: ReturnType<typeof setTimeout> | null = null;
  let notifySoundInterval: ReturnType<typeof setInterval> | null = null;
  let unlistenFileDrop: (() => void) | null = null;

  const actionPatterns = [
    /Do you want to proceed/i,
    /1\.\s*Yes/,
    /\(y\/n\)/i,
    /\[Y\/n\]/i,
    /\[y\/N\]/i,
    /Press Enter/i,
    /Allow.*Deny/i,
    /approve this/i,
    /permission/i,
    /Yes, and don.t ask/i,
  ];

  function checkNotifyBuffer() {
    const buf = notifyOutputBuffer;
    notifyOutputBuffer = '';
    if (!buf) return;
    if (Date.now() - notifyLastTime < 10000) return;
    if (document.hasFocus()) return;

    if (actionPatterns.some(p => p.test(buf))) {
      notifyLastTime = Date.now();

      // macOS notification
      if (get(agentNotifyEnabled)) {
        import('@tauri-apps/plugin-notification').then(({ isPermissionGranted, requestPermission, sendNotification }) => {
          isPermissionGranted().then(granted => {
            if (!granted) {
              requestPermission().then(perm => {
                if (perm === 'granted') sendNotification({ title: 'Clauge', body: 'Claude needs your input' });
              });
            } else {
              sendNotification({ title: 'Clauge', body: 'Claude needs your input' });
            }
          });
        }).catch(() => {});
      }

      // Dock bounce
      if (get(agentDockBounceEnabled)) {
        import('@tauri-apps/api/window').then(({ getCurrentWindow }) => {
          getCurrentWindow().requestUserAttention(2);
        }).catch(() => {});
      }

      // Sound chime + repeat
      if (get(agentSoundEnabled)) {
        playChime();
        if (notifySoundInterval) clearInterval(notifySoundInterval);
        notifySoundInterval = setInterval(() => {
          if (document.hasFocus()) {
            clearInterval(notifySoundInterval!);
            notifySoundInterval = null;
            return;
          }
          playChime();
        }, 3000);
      }
    }
  }

  function playChime() {
    try {
      const ctx = new (window.AudioContext || (window as any).webkitAudioContext)();
      const osc1 = ctx.createOscillator();
      const osc2 = ctx.createOscillator();
      const gain = ctx.createGain();
      osc1.frequency.value = 880;
      osc2.frequency.value = 1318.5;
      osc1.type = 'sine';
      osc2.type = 'sine';
      gain.gain.value = 0.15;
      osc1.connect(gain);
      osc2.connect(gain);
      gain.connect(ctx.destination);
      osc1.start();
      osc2.start();
      gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.3);
      setTimeout(() => { osc1.stop(); osc2.stop(); ctx.close(); }, 400);
    } catch (_) {}
  }

  function handleTerminalOutput(base64Data: string) {
    try {
      const raw = atob(base64Data);
      const text = raw.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, '').replace(/\x1b\][^\x07]*\x07/g, '');
      notifyOutputBuffer += text;
      if (notifyOutputBuffer.length > 500) notifyOutputBuffer = notifyOutputBuffer.slice(-500);

      if (!document.hasFocus()) checkNotifyBuffer();
      if (notifyBufferTimer) clearTimeout(notifyBufferTimer);
      notifyBufferTimer = setTimeout(() => checkNotifyBuffer(), 300);
    } catch (_) {}
  }

  async function loadWebGLAddon(term: Terminal) {
    try {
      const { WebglAddon } = await import('@xterm/addon-webgl');
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => { webgl.dispose(); });
      term.loadAddon(webgl);
    } catch (_) {} // Falls back to canvas renderer silently
  }

  function getCurrentTermTheme(): Record<string, string> {
    const app = get(appearance);
    return getTerminalTheme(app.theme, app.accentColor);
  }

  function createTermEntry(sessionId: string): { term: Terminal; fitAddon: FitAddon; container: HTMLDivElement; terminalId: string | null; _exitBuffer?: string } {
    const t = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: '"JetBrains Mono", "Fira Code", "Cascadia Code", "SF Mono", "Menlo", monospace',
      theme: getCurrentTermTheme(),
      scrollback: 10000,
      lineHeight: 1.35,
    });
    const fa = new FitAddon();
    t.loadAddon(fa);

    const container = document.createElement('div');
    container.style.cssText = 'width:100%;height:100%;display:none;';
    terminalEl.appendChild(container);
    t.open(container);
    loadWebGLAddon(t);

    t.onData((data) => {
      const tIds = get(agentTerminalIds);
      const termId = tIds.get(sessionId);
      if (termId) agentWriteToTerminal(termId, data);
    });

    new ResizeObserver(() => {
      if (fa && container.offsetWidth > 0) {
        requestAnimationFrame(() => {
          try { fa.fit(); } catch (_) {}
        });
      }
    }).observe(container);

    const entry = { term: t, fitAddon: fa, container, terminalId: null as string | null, _exitBuffer: '' };

    agentTerminalMap.update(m => { m.set(sessionId, entry); return new Map(m); });
    return entry;
  }

  function showTermEntry(entry: { term: Terminal; fitAddon: FitAddon; container: HTMLDivElement; terminalId: string | null }) {
    if (activeTermEntry && activeTermEntry !== entry) {
      activeTermEntry.container.style.display = 'none';
      try { activeTermEntry.term.options.scrollback = 1000; } catch (_) {}
    }
    entry.container.style.display = 'block';
    try { entry.term.options.scrollback = 10000; } catch (_) {}
    activeTermEntry = entry;
    requestAnimationFrame(() => { try { entry.fitAddon.fit(); } catch (_) {} });
  }

  function createShellEntry(sessionId: string): { term: Terminal; fitAddon: FitAddon; container: HTMLDivElement; terminalId: string | null } {
    const t = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: '"JetBrains Mono", "Fira Code", "Cascadia Code", "SF Mono", "Menlo", monospace',
      theme: getCurrentTermTheme(),
      scrollback: 5000,
      lineHeight: 1.35,
    });
    const fa = new FitAddon();
    t.loadAddon(fa);

    const container = document.createElement('div');
    container.style.cssText = 'width:100%;height:100%;display:none;';
    shellEl.appendChild(container);
    t.open(container);
    loadWebGLAddon(t);

    t.onData((data) => {
      const sIds = get(agentShellIds);
      const shellId = sIds.get(sessionId);
      if (shellId) {
        agentWriteToTerminal(shellId, data).catch(() => {
          // Shell process died
          agentShellIds.update(m => { m.delete(sessionId); return new Map(m); });
          agentShellOpen.set(false);
          refitAll();
        });
      }
    });

    new ResizeObserver(() => {
      if (fa && container.offsetWidth > 0) {
        requestAnimationFrame(() => { try { fa.fit(); } catch (_) {} });
      }
    }).observe(container);

    const sEntry = { term: t, fitAddon: fa, container, terminalId: null as string | null };
    agentShellMap.update(m => { m.set(sessionId, sEntry); return new Map(m); });
    return sEntry;
  }

  function showShellEntry(sEntry: { term: Terminal; fitAddon: FitAddon; container: HTMLDivElement; terminalId: string | null }) {
    if (activeShellEntry && activeShellEntry !== sEntry) {
      activeShellEntry.container.style.display = 'none';
      try { activeShellEntry.term.options.scrollback = 500; } catch (_) {}
    }
    sEntry.container.style.display = 'block';
    try { sEntry.term.options.scrollback = 5000; } catch (_) {}
    activeShellEntry = sEntry;
    requestAnimationFrame(() => { try { sEntry.fitAddon.fit(); } catch (_) {} });
  }

  function refitAll() {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (activeTermEntry?.fitAddon) {
          try {
            activeTermEntry.fitAddon.fit();
            const tIds = get(agentTerminalIds);
            const session = get(activeAgentSession);
            if (session) {
              const termId = tIds.get(session.id);
              if (termId) {
                const dims = activeTermEntry.fitAddon.proposeDimensions();
                if (dims) agentResizeTerminal(termId, dims.cols, dims.rows).catch(() => {});
              }
            }
          } catch (_) {}
        }
        if (activeShellEntry?.fitAddon) {
          try {
            activeShellEntry.fitAddon.fit();
            const sIds = get(agentShellIds);
            const session = get(activeAgentSession);
            if (session) {
              const shellId = sIds.get(session.id);
              if (shellId) {
                const dims = activeShellEntry.fitAddon.proposeDimensions();
                if (dims) agentResizeTerminal(shellId, dims.cols, dims.rows).catch(() => {});
              }
            }
          } catch (_) {}
        }
      });
    });
  }

  async function spawnShellForSession(session: any) {
    if (!shellEl) return;
    const sMap = get(agentShellMap);
    let sEntry = sMap.get(session.id);
    const sIds = get(agentShellIds);

    if (sEntry && sIds.get(session.id)) {
      showShellEntry(sEntry);
      return;
    }
    if (!sEntry) {
      sEntry = createShellEntry(session.id);
    } else {
      sEntry.term.clear();
    }
    showShellEntry(sEntry);

    const projectPath = session.worktreePath || session.projectPath;
    const channel = new Channel();
    channel.onmessage = (msg: any) => {
      if (!msg.data) return;
      const bytes = Uint8Array.from(atob(msg.data), (c: string) => c.charCodeAt(0));
      sEntry!.term.write(bytes);
    };
    try {
      const shellTermId = await agentSpawnShell(projectPath, channel);
      agentShellIds.update(m => { m.set(session.id, shellTermId); return new Map(m); });
    } catch (e) {
      sEntry.term.write(`\r\nFailed to spawn shell: ${e}\r\n`);
    }
  }

  function stopContextUsagePolling() {
    if (contextUsageInterval) {
      clearInterval(contextUsageInterval);
      contextUsageInterval = null;
    }
  }

  function startContextUsagePolling(session: any) {
    stopContextUsagePolling();
    const projectPath = session.worktreePath || session.projectPath;
    contextUsageInterval = setInterval(() => {
      const s = get(activeAgentSession);
      if (!s || s.id !== session.id) { stopContextUsagePolling(); return; }
      if (s.claudeSessionId) {
        refreshAgentContextUsage(s.id, projectPath, s.claudeSessionId);
      }
    }, 5000);
  }

  async function selectSession(session: any) {
    if (!session || !terminalEl) return;
    currentSessionId = session.id;

    const tMap = get(agentTerminalMap);
    let entry = tMap.get(session.id);
    const tIds = get(agentTerminalIds);

    if (entry && tIds.get(session.id)) {
      showTermEntry(entry);
      if (session.claudeSessionId) startContextUsagePolling(session);
      if (get(agentShellOpen)) spawnShellForSession(session);
      refreshAgentGitStatus();
      return;
    }

    // Create or reuse terminal entry
    if (!entry) {
      entry = createTermEntry(session.id);
    } else {
      entry.term.clear();
      entry.term.write('\r\n\x1b[2mResuming session...\x1b[0m\r\n\r\n');
    }
    showTermEntry(entry);

    try {
      await agentUpdateLastUsed(session.id);

      let spawnPath = session.worktreePath || session.projectPath;

      // Inject attached contexts into CLAUDE.md before spawning
      try {
        const sessionContexts = await agentGetSessionContexts(session.id);
        if (sessionContexts.length > 0) {
          const contextIds = sessionContexts.map((c: any) => c.id);
          await agentInjectContexts(spawnPath, contextIds);
        }
      } catch (_) {}

      // Auto-create worktree for new sessions in git repos
      if (!session.worktreePath && !session.claudeSessionId) {
        try {
          const isGit = await agentIsGitRepo(session.projectPath);
          if (isGit) {
            const rawBranch = `clauge/${session.purpose.toLowerCase().replace(/\s+/g, '-')}-${session.title.toLowerCase().replace(/\s+/g, '-')}`;
            const branchName = rawBranch.replace(/[^a-zA-Z0-9/_\-.]/g, '').replace(/\.{2,}/g, '.').replace(/\.lock/g, '');
            const worktreePath = await agentCreateWorktree(session.projectPath, branchName);
            spawnPath = worktreePath;
            await agentUpdateWorktree(session.id, worktreePath, branchName);
            session.worktreePath = worktreePath;
            session.worktreeBranch = branchName;
            await loadAgentSessions();
          }
        } catch (e) {
          console.warn('Worktree creation failed, using original path:', e);
        }
      }

      // Get existing session IDs BEFORE spawning
      let existingSessionIds: string[] = [];
      if (!session.claudeSessionId) {
        try {
          const existing = await agentDiscoverSessions(spawnPath);
          existingSessionIds = existing.map((s: any) => s.sessionId);
        } catch (_) {}
      }

      let outputReceived = false;
      let activityTimer: ReturnType<typeof setTimeout> | null = null;
      const sessionId = session.id;
      const onOutput = new Channel();

      onOutput.onmessage = (payload: any) => {
        // Write data to terminal
        if (entry!.term) {
          try {
            const binary = atob(payload.data);
            const bytes = new Uint8Array(binary.length);
            for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
            entry!.term.write(bytes);
          } catch (_) {}
        }

        // Check for action-required prompts and notify
        handleTerminalOutput(payload.data);

        // Session exit detection — buffer last 500 chars
        if (!entry!._exitBuffer) entry!._exitBuffer = '';
        try {
          const text = atob(payload.data);
          entry!._exitBuffer += text;
          if (entry!._exitBuffer.length > 500) entry!._exitBuffer = entry!._exitBuffer.slice(-500);
          // Strip ANSI codes
          const clean = entry!._exitBuffer
            .replace(/\x1b\[[0-9;]*[a-zA-Z]/g, '')
            .replace(/\x1b\][^\x07]*\x07/g, '');

          if (/Resume this session with:/.test(clean) || /claude --resume [a-f0-9-]+/.test(clean)) {
            agentTerminalIds.update(m => { m.delete(sessionId); return new Map(m); });
            entry!._exitBuffer = '';
            const resumeMatch = clean.match(/claude --resume ([a-f0-9-]+)/);
            if (resumeMatch && !session.claudeSessionId) {
              const extractedSessionId = resumeMatch[1];
              agentUpdateSessionId(session.id, extractedSessionId).then(() => {
                session.claudeSessionId = extractedSessionId;
                loadAgentSessions();
              }).catch(() => {});
            }
            agentSessionActivity.update(m => { m.set(sessionId, 'done'); return new Map(m); });

            // Auto-switch to next running session (Feature 3)
            if (!_suppressExit) {
              const currentActive = get(activeAgentSession);
              if (currentActive?.id === sessionId) {
                const activity = get(agentSessionActivity);
                const sessions = get(agentSessions);
                const nextRunning = sessions.find(s => s.id !== sessionId && activity.get(s.id) === 'running');
                if (nextRunning) {
                  activeAgentSession.set(nextRunning);
                }
              }
            }
            _suppressExit = false;
          }
        } catch (_) {}

        // Track activity for background sessions
        const currentSession = get(activeAgentSession);
        if (currentSession?.id !== sessionId) {
          agentSessionActivity.update(m => { m.set(sessionId, 'running'); return new Map(m); });
          if (activityTimer) clearTimeout(activityTimer);
          activityTimer = setTimeout(() => {
            const act = get(agentSessionActivity);
            if (act.get(sessionId) === 'running') {
              agentSessionActivity.update(m => { m.set(sessionId, 'done'); return new Map(m); });
            }
          }, 2000);
        }

        // Capture session ID — retry every 3s until found (up to 30s)
        if (!outputReceived && !session.claudeSessionId) {
          outputReceived = true;
          let attempts = 0;
          const captureInterval = setInterval(async () => {
            attempts++;
            if (attempts > 10 || session.claudeSessionId) { clearInterval(captureInterval); return; }
            try {
              const allSessions = await agentDiscoverSessions(spawnPath);
              const newSession = allSessions.find((s: any) => !existingSessionIds.includes(s.sessionId));
              if (newSession) {
                await agentUpdateSessionId(session.id, newSession.sessionId);
                session.claudeSessionId = newSession.sessionId;
                await loadAgentSessions();
                clearInterval(captureInterval);

                // Start context usage polling now that we have a session ID
                if (contextUsageInterval) clearInterval(contextUsageInterval);
                contextUsageInterval = setInterval(() => {
                  const s = get(activeAgentSession);
                  if (s?.claudeSessionId) {
                    const path = s.worktreePath || s.projectPath;
                    refreshAgentContextUsage(s.id, path, s.claudeSessionId);
                  }
                }, 5000);
              }
            } catch (_) {}
          }, 3000);
        }
      };

      // Flatten prompt to single line for shell compatibility (matches original Clauge)
      // Use frontend purpose prompt for fixed purposes, fall back to stored prompt for Custom
      const rawPrompt = getPurposePrompt(session.purpose) || session.contextPrompt || '';
      const purposePrompt = rawPrompt.replace(/\n+/g, ' ').replace(/\s+/g, ' ').trim();

      const termId = await agentSpawnTerminal({
        sessionId: session.claudeSessionId || undefined,
        projectPath: spawnPath,
        contextPrompt: purposePrompt || undefined,
        skipPermissions: session.skipPermissions === 1 || undefined,
        gitName: session.gitName || undefined,
        gitEmail: session.gitEmail || undefined,
        onOutput,
      });
      agentTerminalIds.update(m => { m.set(session.id, termId); return new Map(m); });

      // Start context usage polling (Feature 2)
      startContextUsagePolling(session);

      // Fit + resize PTY
      requestAnimationFrame(() => {
        try {
          entry!.fitAddon.fit();
          const dims = entry!.fitAddon.proposeDimensions();
          if (dims) agentResizeTerminal(termId, dims.cols, dims.rows).catch(() => {});
        } catch (_) {}
      });

      if (get(agentShellOpen)) spawnShellForSession(session);
      refreshAgentGitStatus();
    } catch (e) {
      entry.term.write(`\r\nFailed to spawn terminal: ${e}\r\n`);
    }
  }

  // Draggable divider between main terminal and shell
  function handleDividerMousedown(e: MouseEvent) {
    e.preventDefault();
    dragging = true;

    const startY = e.clientY;
    const startHeight = mainHeight;
    const rect = wrapperEl.getBoundingClientRect();

    function onMove(ev: MouseEvent) {
      const deltaY = ev.clientY - startY;
      const pct = startHeight + (deltaY / rect.height) * 100;
      mainHeight = Math.max(20, Math.min(80, pct));

      if (!fitTimer) {
        fitTimer = setTimeout(() => {
          fitTimer = null;
          try { activeTermEntry?.fitAddon?.fit(); } catch (_) {}
          try { activeShellEntry?.fitAddon?.fit(); } catch (_) {}
        }, 100);
      }
    }

    function onUp() {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      dragging = false;
      if (fitTimer) { clearTimeout(fitTimer); fitTimer = null; }
      refitAll();
    }

    document.body.style.cursor = 'row-resize';
    document.body.style.userSelect = 'none';
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  }

  // React to theme changes — update all terminal instances
  const unsubAppearance = appearance.subscribe((app) => {
    if (!app) return;
    const termTheme = getTerminalTheme(app.theme, app.accentColor);
    const tMap = get(agentTerminalMap);
    for (const [, entry] of tMap) {
      if (entry?.term) entry.term.options.theme = termTheme;
    }
    const sMap = get(agentShellMap);
    for (const [, entry] of sMap) {
      if (entry?.term) entry.term.options.theme = termTheme;
    }
  });

  // React to session changes
  const unsubSession = activeAgentSession.subscribe((session) => {
    if (session && session.id !== currentSessionId) {
      // Wait for DOM
      requestAnimationFrame(() => selectSession(session));
    } else if (!session) {
      currentSessionId = null;
      if (activeTermEntry) {
        activeTermEntry.container.style.display = 'none';
        activeTermEntry = null;
      }
      if (activeShellEntry) {
        activeShellEntry.container.style.display = 'none';
        activeShellEntry = null;
      }
    }
  });

  // React to shell toggle
  const unsubShell = agentShellOpen.subscribe((open) => {
    const session = get(activeAgentSession);
    if (open && session) {
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          spawnShellForSession(session);
          refitAll();
        });
      });
    } else {
      if (activeShellEntry) {
        activeShellEntry.container.style.display = 'none';
        activeShellEntry = null;
      }
      refitAll();
    }
  });

  // Event handler for reset-session (Feature 4)
  function handleResetSession(e: Event) {
    const detail = (e as CustomEvent).detail;
    if (!detail?.session) return;
    const session = detail.session;
    _suppressExit = true;

    // Kill terminal
    const tIds = get(agentTerminalIds);
    const termId = tIds.get(session.id);
    if (termId) agentKillTerminal(termId).catch(() => {});

    // Kill shell
    const sIds = get(agentShellIds);
    const shellId = sIds.get(session.id);
    if (shellId) agentKillTerminal(shellId).catch(() => {});

    // Clear session ID
    agentUpdateSessionId(session.id, '').catch(() => {});
    session.claudeSessionId = null;

    // Remove from maps
    agentTerminalIds.update(m => { m.delete(session.id); return new Map(m); });
    agentShellIds.update(m => { m.delete(session.id); return new Map(m); });

    // Remove terminal entry so selectSession creates fresh one
    const tMap = get(agentTerminalMap);
    const entry = tMap.get(session.id);
    if (entry) {
      entry.container.remove();
      entry.term.dispose();
      agentTerminalMap.update(m => { m.delete(session.id); return new Map(m); });
    }

    // Remove shell entry
    const sMap = get(agentShellMap);
    const sEntry = sMap.get(session.id);
    if (sEntry) {
      sEntry.container.remove();
      sEntry.term.dispose();
      agentShellMap.update(m => { m.delete(session.id); return new Map(m); });
    }

    // Reset current session ID so selectSession processes it
    currentSessionId = null;
    activeTermEntry = null;
    activeShellEntry = null;

    loadAgentSessions().then(() => {
      selectSession(session);
    });
  }

  // Event handler for delete-session (Feature 5)
  async function handleDeleteSession(e: Event) {
    const detail = (e as CustomEvent).detail;
    if (!detail?.session) return;
    const session = detail.session;

    _suppressExit = true;

    // Kill terminal
    const tIds = get(agentTerminalIds);
    const termId = tIds.get(session.id);
    if (termId) await agentKillTerminal(termId).catch(() => {});

    // Kill shell
    const sIds = get(agentShellIds);
    const shellId = sIds.get(session.id);
    if (shellId) await agentKillTerminal(shellId).catch(() => {});

    // Cleanup terminal entry
    const tMap = get(agentTerminalMap);
    const entry = tMap.get(session.id);
    if (entry) {
      entry.container.remove();
      entry.term.dispose();
      agentTerminalMap.update(m => { m.delete(session.id); return new Map(m); });
    }

    // Cleanup shell entry
    const sMap = get(agentShellMap);
    const sEntry = sMap.get(session.id);
    if (sEntry) {
      sEntry.container.remove();
      sEntry.term.dispose();
      agentShellMap.update(m => { m.delete(session.id); return new Map(m); });
    }

    agentTerminalIds.update(m => { m.delete(session.id); return new Map(m); });
    agentShellIds.update(m => { m.delete(session.id); return new Map(m); });

    // Remove worktree if exists
    if (session.worktreePath) {
      await agentRemoveWorktree(session.projectPath, session.worktreePath).catch(() => {});
    }

    // Delete from DB
    await agentDeleteSession(session.id).catch(() => {});

    // If this was the active session, clear it
    const currentActive = get(activeAgentSession);
    if (currentActive?.id === session.id) {
      currentSessionId = null;
      activeTermEntry = null;
      activeShellEntry = null;
      activeAgentSession.set(null);
    }

    await loadAgentSessions();
  }

  onMount(async () => {
    // Load notification preferences from settings
    try {
      const [notify, sound, dock] = await Promise.all([
        getSetting('agent_notify_enabled'),
        getSetting('agent_sound_enabled'),
        getSetting('agent_dock_bounce_enabled'),
      ]);
      agentNotifyEnabled.set(notify === 'true');
      agentSoundEnabled.set(sound === 'true');
      agentDockBounceEnabled.set(dock === 'true');
    } catch (_) {}

    // Listen for reset-session and delete-session events
    window.addEventListener('agent:reset-session', handleResetSession);
    window.addEventListener('agent:delete-session', handleDeleteSession);

    // File drag-and-drop: write dropped file paths into the active terminal
    try {
      const win = getCurrentWindow();
      win.onDragDropEvent((event) => {
        if (event.payload.type === 'drop') {
          const paths = event.payload.paths;
          if (!paths?.length) return;
          const session = get(activeAgentSession);
          if (!session) return;
          const tIds = get(agentTerminalIds);
          const termId = tIds.get(session.id);
          if (termId) {
            const pathStr = paths.map((p: string) => p.includes(' ') ? `"${p}"` : p).join(' ');
            agentWriteToTerminal(termId, pathStr + ' ');
          }
        }
      }).then((unlisten) => {
        unlistenFileDrop = unlisten;
      });
    } catch (_) {}
  });

  onDestroy(() => {
    unsubSession();
    unsubShell();
    unsubAppearance();
    stopContextUsagePolling();
    if (notifyBufferTimer) clearTimeout(notifyBufferTimer);
    if (notifySoundInterval) clearInterval(notifySoundInterval);
    window.removeEventListener('agent:reset-session', handleResetSession);
    window.removeEventListener('agent:delete-session', handleDeleteSession);
    if (unlistenFileDrop) unlistenFileDrop();
  });
</script>

{#if $activeAgentSession}
  <div class="agent-panel" bind:this={wrapperEl}>
    <div class="agent-terminal-main" style="height:{$agentShellOpen ? mainHeight + '%' : '100%'}">
      <div class="agent-terminal-container" bind:this={terminalEl}></div>
    </div>

    {#if $agentShellOpen}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="agent-divider"
        class:active={dragging}
        onmousedown={handleDividerMousedown}
      ></div>

      <div class="agent-shell-panel" style="height:{100 - mainHeight}%">
        <div class="agent-shell-container" bind:this={shellEl}></div>
      </div>
    {/if}
  </div>
{:else}
  <div class="agent-empty">
    <svg viewBox="0 0 24 24" width="36" height="36">
      <rect x="3" y="3" width="18" height="18" rx="3" stroke="var(--t4)" fill="none" stroke-width="1.2"/>
      <path d="M7 8h10M7 12h6M7 16h8" stroke="var(--t4)" fill="none" stroke-width="1.2" stroke-linecap="round"/>
    </svg>
    <span>Select or create a session to start</span>
  </div>
{/if}

<style>
  .agent-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }

  .agent-terminal-main {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-height: 60px;
  }

  .agent-terminal-container {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .agent-divider {
    height: 4px;
    flex-shrink: 0;
    background: var(--b1);
    cursor: row-resize;
    position: relative;
    transition: background 0.12s;
  }
  .agent-divider:hover,
  .agent-divider.active {
    background: var(--acc);
  }

  .agent-shell-panel {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-height: 60px;
  }

  .agent-shell-container {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .agent-empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--t3);
    font-size: 13px;
    font-family: var(--ui);
  }
</style>
