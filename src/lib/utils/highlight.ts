import hljs from 'highlight.js';

const EXT_TO_LANG: Record<string, string> = {
	js: 'javascript', ts: 'typescript', jsx: 'javascript', tsx: 'typescript',
	py: 'python', rs: 'rust', go: 'go', c: 'c', cpp: 'cpp', h: 'c',
	java: 'java', rb: 'ruby', swift: 'swift', kt: 'kotlin',
	svelte: 'xml', vue: 'xml', html: 'xml', xml: 'xml',
	css: 'css', scss: 'scss', less: 'less',
	json: 'json', yaml: 'yaml', yml: 'yaml', toml: 'ini',
	sh: 'bash', bash: 'bash', zsh: 'bash', fish: 'bash',
	sql: 'sql', graphql: 'graphql',
	md: 'markdown', txt: '', csv: '', log: '',
	makefile: 'makefile', dockerfile: 'dockerfile',
	ini: 'ini', cfg: 'ini', conf: 'ini',
	tf: 'terraform', tfvars: 'terraform', hcl: 'terraform',
	nix: 'nix',
};

export function detectLanguage(filename: string): string | undefined {
	const ext = filename.split('.').pop()?.toLowerCase() ?? '';
	const lang = EXT_TO_LANG[ext];
	if (lang === '') return undefined;
	return lang || undefined;
}

export function highlightCode(code: string, language?: string): string {
	if (language) {
		try {
			return hljs.highlight(code, { language }).value;
		} catch { /* fall through to auto */ }
	}
	try {
		return hljs.highlightAuto(code).value;
	} catch {
		return escapeHtml(code);
	}
}

function escapeHtml(s: string): string {
	return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
