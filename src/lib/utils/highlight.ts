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
	md: 'markdown', txt: 'plaintext', csv: 'plaintext', log: 'plaintext',
	makefile: 'makefile', dockerfile: 'dockerfile',
	ini: 'ini', cfg: 'ini', conf: 'ini',
	tf: 'terraform', tfvars: 'terraform', hcl: 'terraform',
	nix: 'nix',
};

export function detectLanguage(filename: string): string | undefined {
	const name = filename.split(/[\\/]/).pop()?.toLowerCase() ?? '';
	// Terraform state is JSON, not HCL or a language to guess from string values.
	if (/\.tfstate(?:\.backup|\.lock\.info)?$/.test(name)) return 'json';
	const ext = name.split('.').pop() ?? '';
	const lang = Object.hasOwn(EXT_TO_LANG, ext) ? EXT_TO_LANG[ext] : undefined;
	return lang && hljs.getLanguage(lang) ? lang : undefined;
}

export const syntaxLanguages = hljs.listLanguages().map(id => ({
	id, name: hljs.getLanguage(id)?.name ?? id,
})).sort((left, right) => left.name.localeCompare(right.name));

export function highlightCode(code: string, language?: string): string {
	if (language && hljs.getLanguage(language)) {
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

/** Each numbered viewer line is its own DOM fragment. Reopen multiline token
 * spans on the next line so comments/strings retain their syntax and valid HTML. */
export function highlightCodeLines(code: string, language?: string): string[] {
	const spans: string[] = [];
	return highlightCode(code, language).split('\n').map(line => {
		const prefix = spans.join('');
		for (const match of line.matchAll(/<\/?span\b[^>]*>/g)) {
			if (match[0].startsWith('</')) spans.pop();
			else spans.push(match[0]);
		}
		return prefix + line + '</span>'.repeat(spans.length);
	});
}

function escapeHtml(s: string): string {
	return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
