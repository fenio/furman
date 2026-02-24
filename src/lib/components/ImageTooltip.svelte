<script lang="ts">
  interface Props {
    src: string;
    anchorRect: DOMRect;
  }

  let { src, anchorRect }: Props = $props();

  const style = $derived.by(() => {
    const margin = 8;
    const tooltipWidth = 200;
    const tooltipMaxHeight = 200;
    const viewW = window.innerWidth;
    const viewH = window.innerHeight;

    // Prefer right side of row
    let left = anchorRect.right + margin;
    if (left + tooltipWidth > viewW) {
      // Flip to left side
      left = anchorRect.left - tooltipWidth - margin;
    }
    if (left < 0) left = margin;

    // Prefer aligned with top of row
    let top = anchorRect.top;
    if (top + tooltipMaxHeight > viewH) {
      // Flip above
      top = anchorRect.bottom - tooltipMaxHeight;
    }
    if (top < 0) top = margin;

    return `left: ${left}px; top: ${top}px; width: ${tooltipWidth}px;`;
  });
</script>

<div class="image-tooltip" {style}>
  <img {src} alt="Preview" loading="eager" />
</div>

<style>
  .image-tooltip {
    position: fixed;
    z-index: 200;
    background: var(--dialog-bg);
    border: 1px solid var(--border-subtle);
    box-shadow: var(--shadow-dialog);
    border-radius: var(--radius-md);
    padding: 4px;
    pointer-events: none;
    max-height: 200px;
    overflow: hidden;
  }

  .image-tooltip img {
    width: 100%;
    max-height: 192px;
    object-fit: contain;
    border-radius: var(--radius-sm);
    display: block;
  }
</style>
