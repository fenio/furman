class StatusState {
  message = $state('');
  isProgress = $state(false);
  progressPercent = $state(0);
  progressDetail = $state('');

  private clearTimer: ReturnType<typeof setTimeout> | null = null;

  setMessage(msg: string, autoClearMs = 5000) {
    this.isProgress = false;
    this.progressPercent = 0;
    this.progressDetail = '';
    this.message = msg;

    if (this.clearTimer) clearTimeout(this.clearTimer);
    this.clearTimer = setTimeout(() => this.clear(), autoClearMs);
  }

  setProgress(detail: string, percent: number) {
    if (this.clearTimer) {
      clearTimeout(this.clearTimer);
      this.clearTimer = null;
    }
    this.isProgress = true;
    this.progressPercent = Math.min(100, Math.max(0, percent));
    this.progressDetail = detail;
    this.message = '';
  }

  clear() {
    if (this.clearTimer) {
      clearTimeout(this.clearTimer);
      this.clearTimer = null;
    }
    this.message = '';
    this.isProgress = false;
    this.progressPercent = 0;
    this.progressDetail = '';
  }
}

export const statusState = new StatusState();
