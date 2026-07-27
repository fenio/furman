import { EditorView } from '@codemirror/view';
import type { Extension } from '@codemirror/state';
import { syntaxHighlighting, type LanguageSupport } from '@codemirror/language';
import { oneDarkHighlightStyle } from '@codemirror/theme-one-dark';
import { defaultHighlightStyle } from '@codemirror/language';

// Official language imports
import { javascript } from '@codemirror/lang-javascript';
import { python } from '@codemirror/lang-python';
import { rust } from '@codemirror/lang-rust';
import { html } from '@codemirror/lang-html';
import { css } from '@codemirror/lang-css';
import { json } from '@codemirror/lang-json';
import { markdown } from '@codemirror/lang-markdown';
import { xml } from '@codemirror/lang-xml';
import { java } from '@codemirror/lang-java';
import { cpp } from '@codemirror/lang-cpp';
import { sql } from '@codemirror/lang-sql';
import { go } from '@codemirror/lang-go';
import { yaml } from '@codemirror/lang-yaml';

// Legacy modes (wrapped via StreamLanguage)
import { StreamLanguage } from '@codemirror/language';
import { shell } from '@codemirror/legacy-modes/mode/shell';
import { toml } from '@codemirror/legacy-modes/mode/toml';
import { dockerFile } from '@codemirror/legacy-modes/mode/dockerfile';
import { ruby } from '@codemirror/legacy-modes/mode/ruby';
import { kotlin } from '@codemirror/legacy-modes/mode/clike';
import { swift } from '@codemirror/legacy-modes/mode/swift';
import { hcl } from 'codemirror-lang-hcl';
import { nix } from '@replit/codemirror-lang-nix';

// Extension → language factory
const EXT_TO_LANG: Record<string, () => LanguageSupport | Extension> = {
	js: () => javascript(),
	ts: () => javascript({ typescript: true }),
	jsx: () => javascript({ jsx: true }),
	tsx: () => javascript({ jsx: true, typescript: true }),
	py: () => python(),
	rs: () => rust(),
	go: () => go(),
	c: () => cpp(),
	cpp: () => cpp(),
	h: () => cpp(),
	java: () => java(),
	svelte: () => html(),
	vue: () => html(),
	html: () => html(),
	css: () => css(),
	scss: () => css(),
	less: () => css(),
	json: () => json(),
	xml: () => xml(),
	yaml: () => yaml(),
	yml: () => yaml(),
	sql: () => sql(),
	md: () => markdown(),
	// Legacy modes
	sh: () => StreamLanguage.define(shell),
	bash: () => StreamLanguage.define(shell),
	zsh: () => StreamLanguage.define(shell),
	toml: () => StreamLanguage.define(toml),
	dockerfile: () => StreamLanguage.define(dockerFile),
	rb: () => StreamLanguage.define(ruby),
	kt: () => StreamLanguage.define(kotlin),
	swift: () => StreamLanguage.define(swift),
	tf: () => hcl(),
	tfvars: () => hcl(),
	hcl: () => hcl(),
	nix: () => nix(),
};

export function getLanguageExtension(filename: string): LanguageSupport | Extension | null {
	const ext = filename.split('.').pop()?.toLowerCase() ?? '';
	const factory = EXT_TO_LANG[ext];
	return factory ? factory() : null;
}

// Theme that inherits from app CSS variables
export const editorTheme = EditorView.theme({
	'&': {
		backgroundColor: 'var(--bg-primary)',
		color: 'var(--text-primary)',
		fontSize: '13px',
		height: '100%',
	},
	'.cm-content': {
		fontFamily: "'Menlo', 'Consolas', 'Courier New', monospace",
		caretColor: 'var(--text-primary)',
	},
	'.cm-cursor': {
		borderLeftColor: 'var(--text-primary)',
	},
	'&.cm-focused .cm-selectionBackground, .cm-selectionBackground': {
		backgroundColor: 'var(--cursor-bg) !important',
	},
	'.cm-gutters': {
		backgroundColor: 'var(--bg-panel)',
		color: 'var(--text-secondary)',
		borderRight: '1px solid var(--border-subtle)',
		fontFamily: "'Menlo', 'Consolas', 'Courier New', monospace",
	},
	'.cm-activeLineGutter': {
		backgroundColor: 'var(--cursor-bg)',
	},
	'.cm-activeLine': {
		backgroundColor: 'var(--cursor-bg)',
	},
	'.cm-matchingBracket': {
		backgroundColor: 'rgba(110,168,254,0.3)',
		outline: 'none',
	},
	'.cm-panels': {
		backgroundColor: 'var(--bg-header)',
		color: 'var(--text-primary)',
		borderBottom: '1px solid var(--border-subtle)',
	},
	'.cm-searchMatch': {
		backgroundColor: 'rgba(110,168,254,0.25)',
	},
	'.cm-searchMatch.cm-searchMatch-selected': {
		backgroundColor: 'rgba(110,168,254,0.5)',
	},
	'.cm-selectionMatch': {
		backgroundColor: 'transparent',
	},
	'.cm-panel input': {
		backgroundColor: 'var(--bg-surface)',
		color: 'var(--text-primary)',
		border: '1px solid var(--border-subtle)',
	},
	'.cm-panel button': {
		backgroundColor: 'var(--bg-surface)',
		color: 'var(--text-primary)',
		border: '1px solid var(--border-subtle)',
	},
});

export function getSyntaxHighlighting(): Extension {
	const isDark = document.documentElement.getAttribute('data-theme') !== 'light';
	return syntaxHighlighting(isDark ? oneDarkHighlightStyle : defaultHighlightStyle);
}
