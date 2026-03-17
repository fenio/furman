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
		try {
			await writeFileText(path, text);
			if (appState.editorS3ConnectionId) {
				await s3PutText(appState.editorS3ConnectionId, appState.editorS3Key, text);
			} else if (appState.editorSftpConnectionId) {
				await sftpPutText(appState.editorSftpConnectionId, appState.editorSftpPath, text);
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
			appState.showConfirm('File has been modified. Discard changes?', () => {
				appState.closeModal();
				onClose();
			});
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
</style>
