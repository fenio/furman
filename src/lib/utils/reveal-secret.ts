/** Add a local, accessible reveal toggle without changing the bound value. */
let fieldId = 0;
export function revealSecret(input: HTMLInputElement, enabled = true) {
  if (!enabled) return {};
  const wrapper = document.createElement('span');
  wrapper.style.cssText = 'display:flex;align-items:center;gap:6px;width:100%;min-width:0';
  input.before(wrapper);
  wrapper.append(input);
  const button = document.createElement('button');
  const originalId = input.id;
  const originalMinWidth = input.style.minWidth;
  if (!input.id) input.id = `credential-field-${++fieldId}`;
  input.style.minWidth = '0';
  button.setAttribute('aria-controls', input.id);
  button.type = 'button';
  button.style.cssText = 'flex-shrink:0;background:transparent;color:var(--text-secondary);border:1px solid var(--border-subtle);border-radius:4px;padding:6px;cursor:pointer;line-height:0';
  button.innerHTML = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/></svg>';
  wrapper.append(button);
  function refresh() {
    const visible = input.type === 'text';
    button.setAttribute('aria-label', visible ? 'Hide credential' : 'Show credential');
    button.setAttribute('aria-pressed', String(visible));
    button.title = visible ? 'Hide credential' : 'Show credential';
    button.disabled = input.disabled;
  }
  function toggle() {
    input.type = input.type === 'password' ? 'text' : 'password';
    refresh();
    input.focus();
  }
  button.addEventListener('click', toggle);
  const observer = new MutationObserver(refresh);
  observer.observe(input, { attributes: true, attributeFilter: ['disabled', 'type'] });
  refresh();
  return { destroy() {
    observer.disconnect();
    button.removeEventListener('click', toggle);
    input.type = 'password';
    input.id = originalId;
    input.style.minWidth = originalMinWidth;
    wrapper.before(input);
    wrapper.remove();
  } };
}
