<script lang="ts">
	import { readFileText, writeFileText } from '$lib/services/tauri';
	import { s3PutText } from '$lib/services/s3';
	import { sftpPutText } from '$lib/services/sftp';
	import { appState } from '$lib/state/app.svelte';
	import { onMount, untrack } from 'svelte';
	import { EditorView, keymap } from '@codemirror/view';
	import { EditorState, Prec } from '@codemirror/state';
	import { basicSetup } from 'codemirror';
	import { getLanguageExtension, editorTheme, getSyntaxHighlighting } from '$lib/utils/codemirror';

	interface Props {
		path: string;
		onClose: () => void;
	}

	let { path, onClose }: Props = $props();

	let content = $state('');
	let originalContent = $state('');
	let dirty = $state(false);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let saving = $state(false);
	let showDiscardConfirm = $state(false);
	let editorContainer: HTMLDivElement | undefined = $state(undefined);
	let view: EditorView | undefined = $state(undefined);

	const fileName = $derived(path.split('/').pop() ?? path);

	onMount(async () => {
		try {
			content = await readFileText(path);
			originalContent = content;
		} catch (err: unknown) {
			error = err instanceof Error ? err.message : String(err);
		} finally {
			loading = false;
		}
	});

	$effect(() => {
		appState.editorDirty = dirty;
	});

	// Create CodeMirror view once content is loaded and container is ready.
	// Only track loading + editorContainer; read everything else untracked
	// so the effect doesn't re-run (and destroy the view) on content changes.
	$effect(() => {
		if (loading || !editorContainer) return;

		return untrack(() => {
			if (view) return;
			if (error && !content) return;

			const lang = getLanguageExtension(fileName);
			const extensions = [
				basicSetup,
				editorTheme,
				getSyntaxHighlighting(),
				Prec.highest(
					keymap.of([
						{
							key: 'Mod-s',
							run: () => {
								save();
								return true;
							},
						},
						{
							key: 'F2',
							run: () => {
								save();
								return true;
							},
						},
					]),
				),
				EditorView.updateListener.of((update) => {
					if (update.docChanged) {
						const current = update.state.doc.toString();
						dirty = current !== originalContent;
					}
				}),
			];
			if (lang) extensions.push(lang);

			const state = EditorState.create({
				doc: content,
				extensions,
			});

			view = new EditorView({
				state,
				parent: editorContainer,
			});

			view.focus();

			return () => {
				view?.destroy();
				view = undefined;
			};
		});
	});

	async function save() {
		if (saving || !view) return;
		saving = true;
		const text = view.state.doc.toString();
		const localPath = path;
		const s3Target = appState.editorS3ConnectionId
			? { connectionId: appState.editorS3ConnectionId, path: appState.editorS3Key }
			: null;
		const sftpTarget = appState.editorSftpConnectionId
			? { connectionId: appState.editorSftpConnectionId, path: appState.editorSftpPath }
			: null;
		try {
			if (s3Target && sftpTarget) {
				throw new Error('Editor has multiple remote destinations');
			}
			await writeFileText(localPath, text);
			if (s3Target) {
				await s3PutText(s3Target.connectionId, s3Target.path, text);
			} else if (sftpTarget) {
				await sftpPutText(sftpTarget.connectionId, sftpTarget.path, text);
			}
			originalContent = text;
			content = text;
			dirty = false;
		} catch (err: unknown) {
			error = err instanceof Error ? err.message : String(err);
		} finally {
			saving = false;
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (showDiscardConfirm) {
			if (e.key === 'Escape') {
				e.preventDefault();
				e.stopPropagation();
				showDiscardConfirm = false;
			}
			return;
		}
		if (e.key === 'Escape' || ((e.ctrlKey || e.metaKey) && e.key === 'w')) {
			e.preventDefault();
			e.stopPropagation();
			handleClose();
			return;
		}

		if ((e.ctrlKey || e.metaKey) && e.key === 's') {
			e.preventDefault();
			e.stopPropagation();
			save();
			return;
		}

		if (e.key === 'F2') {
			e.preventDefault();
			e.stopPropagation();
			save();
			return;
		}
	}

	function handleClose() {
		if (dirty) {
			showDiscardConfirm = true;
		} else {
			onClose();
		}
	}
</script>

<div
	class="editor-overlay no-select"
	onkeydown={handleKeydown}
	role="dialog"
	aria-modal="true"
	tabindex="-1"
>
	<!-- Header -->
	<div class="editor-header">
		<span class="editor-filename">{fileName}</span>
		{#if dirty}
			<span class="editor-modified">[Modified]</span>
		{/if}
		{#if saving}
			<span class="editor-saving">[Saving...]</span>
		{/if}
		<span class="editor-help">⌘S/F2=Save  ⌘F=Search  ESC=Close</span>
	</div>

	<!-- Content -->
	{#if loading}
		<div class="editor-loading">Loading...</div>
	{:else if error && !content}
		<div class="editor-error">Error: {error}</div>
	{:else}
		<div class="editor-body" bind:this={editorContainer}></div>
	{/if}

	{#if error && content}
		<div class="editor-status-error">Error: {error}</div>
	{/if}

	{#if showDiscardConfirm}
		<div class="discard-overlay" role="alertdialog" aria-modal="true" aria-label="Discard changes">
			<div class="discard-dialog">
				<div>File has been modified. Discard changes?</div>
				<div class="discard-actions">
					<button onclick={() => { showDiscardConfirm = false; onClose(); }}>Discard</button>
					<button class="primary" onclick={() => { showDiscardConfirm = false; view?.focus(); }}>Keep editing</button>
				</div>
			</div>
		</div>
	{/if}
</div>

<style>
	.editor-overlay {
		position: fixed;
		top: 0;
		left: 0;
		right: 0;
		bottom: 0;
		background: var(--bg-primary);
		display: flex;
		flex-direction: column;
		z-index: 200;
	}

	.editor-header {
		display: flex;
		gap: 2ch;
		background: var(--bg-header);
		color: var(--text-primary);
		padding: 4px 12px;
		flex: 0 0 auto;
		border-bottom: 1px solid var(--border-subtle);
	}

	.editor-filename {
		font-weight: 600;
	}

	.editor-modified {
		color: var(--error-color);
	}

	.editor-saving {
		color: var(--text-accent);
	}

	.editor-help {
		margin-left: auto;
		font-size: 12px;
		color: var(--text-secondary);
	}

	.editor-loading,
	.editor-error {
		padding: 16px;
		color: var(--text-secondary);
	}

	.editor-error {
		color: var(--error-color);
	}

	.editor-body {
		flex: 1 1 0;
		min-height: 0;
		overflow: hidden;
	}

	.editor-status-error {
		background: var(--error-bg);
		color: var(--error-color);
		padding: 2px 12px;
		flex: 0 0 auto;
		font-size: 12px;
		border-top: 1px solid var(--border-subtle);
	}

	.discard-overlay {
		position: absolute;
		inset: 0;
		display: grid;
		place-items: center;
		background: color-mix(in srgb, var(--bg-primary) 70%, transparent);
		z-index: 1;
	}

	.discard-dialog {
		min-width: 320px;
		padding: 18px;
		background: var(--bg-secondary);
		border: 1px solid var(--border-active);
		border-radius: var(--radius-md);
		box-shadow: var(--shadow-dialog);
	}

	.discard-actions {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
		margin-top: 16px;
	}

	.discard-actions button {
		padding: 5px 12px;
		border: 1px solid var(--border-subtle);
		border-radius: var(--radius-sm);
		background: var(--bg-tertiary);
		color: var(--text-primary);
	}

	.discard-actions button.primary {
		border-color: var(--text-accent);
	}
</style>
