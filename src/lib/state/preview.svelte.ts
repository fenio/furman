class PreviewState {
  visible = $state(false);

  toggle() {
    this.visible = !this.visible;
  }
}

export const previewState = new PreviewState();
