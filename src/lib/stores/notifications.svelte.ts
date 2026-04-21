class NotificationStore {
  private outputBuffer = '';
  private lastNotifyTime = 0;
  private soundRepeatInterval: ReturnType<typeof setInterval> | null = null;
  private bufferTimer: ReturnType<typeof setTimeout> | null = null;

  checkForActionPrompt(base64Data: string, sessionTitle: string) {
    // Decode base64 to text, strip ANSI escape codes
    const raw = atob(base64Data);
    const text = raw.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, '').replace(/\x1b\][^\x07]*\x07/g, '');
    this.outputBuffer += text;

    // Check on every chunk if unfocused (timers may be throttled in background)
    if (!document.hasFocus()) this.checkBuffer(sessionTitle);
    // Also debounce for when data arrives in small chunks
    if (this.bufferTimer) clearTimeout(this.bufferTimer);
    this.bufferTimer = setTimeout(() => this.checkBuffer(sessionTitle), 300);
  }

  private checkBuffer(sessionTitle: string) {
    const buf = this.outputBuffer;
    this.outputBuffer = '';
    if (!buf) return;

    // Throttle — max one notification per 10 seconds
    if (Date.now() - this.lastNotifyTime < 10000) return;

    // Only notify if window is not focused
    if (document.hasFocus()) return;

    const patterns = [
      /Do you want to proceed/i,
      /1\.\s*Yes/,
      /\(y\/n\)/i,
      /\[Y\/n\]/i,
      /\[y\/N\]/i,
      /Press Enter/i,
      /Allow.*Deny/i,
      /approve this/i,
      /Yes, and don.t ask/i,
    ];

    if (patterns.some(p => p.test(buf))) {
      this.lastNotifyTime = Date.now();
      this.sendActionNotification(sessionTitle);
      // Bounce Dock icon
      import("@tauri-apps/api/window").then(({ getCurrentWindow, UserAttentionType }) => {
        getCurrentWindow().requestUserAttention(UserAttentionType.Critical);
      }).catch(() => {});
      this.playNotificationSound();
      // Repeat sound every 3s until focused
      if (this.soundRepeatInterval) clearInterval(this.soundRepeatInterval);
      this.soundRepeatInterval = setInterval(() => {
        if (document.hasFocus()) {
          clearInterval(this.soundRepeatInterval!);
          this.soundRepeatInterval = null;
          return;
        }
        this.playNotificationSound();
      }, 3000);
    }
  }

  playNotificationSound() {
    try {
      const ctx = new (window.AudioContext || (window as any).webkitAudioContext)();
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.connect(gain);
      gain.connect(ctx.destination);
      osc.frequency.value = 880; // A5 note
      osc.type = 'sine';
      gain.gain.setValueAtTime(0.08, ctx.currentTime); // Very quiet
      gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.3);
      osc.start(ctx.currentTime);
      osc.stop(ctx.currentTime + 0.3);
      // Play a second tone for a pleasant chime
      const osc2 = ctx.createOscillator();
      const gain2 = ctx.createGain();
      osc2.connect(gain2);
      gain2.connect(ctx.destination);
      osc2.frequency.value = 1320; // E6 note
      osc2.type = 'sine';
      gain2.gain.setValueAtTime(0.05, ctx.currentTime + 0.1);
      gain2.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.4);
      osc2.start(ctx.currentTime + 0.1);
      osc2.stop(ctx.currentTime + 0.4);
    } catch (_) {}
  }

  async sendActionNotification(sessionTitle: string) {
    try {
      const { isPermissionGranted, requestPermission, sendNotification } = await import("@tauri-apps/plugin-notification");
      let granted = await isPermissionGranted();
      if (!granted) {
        const permission = await requestPermission();
        granted = permission === 'granted';
      }
      if (granted) {
        sendNotification({ title: `Action Required`, body: `${sessionTitle} — Claude is waiting for your input` });
      }
    } catch (_) {}
  }
}

export const notifications = new NotificationStore();
