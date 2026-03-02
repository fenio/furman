import js from '@eslint/js';
import ts from 'typescript-eslint';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';

export default ts.config(
	js.configs.recommended,
	...ts.configs.recommended,
	...svelte.configs['flat/recommended'],
	{
		languageOptions: {
			globals: {
				...globals.browser,
				...globals.node,
			},
		},
	},
	{
		rules: {
			// Catch bugs
			'no-console': 'warn',
			'no-debugger': 'warn',
			eqeqeq: ['error', 'always', { null: 'ignore' }],
			'no-var': 'error',
			'prefer-const': 'error',

			// TypeScript
			'@typescript-eslint/no-unused-vars': [
				'warn',
				{ argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
			],
			'@typescript-eslint/no-explicit-any': 'warn',
		},
	},
	// Svelte overrides (must come AFTER general rules)
	{
		files: ['**/*.svelte', '**/*.svelte.ts'],
		languageOptions: {
			parserOptions: {
				parser: ts.parser,
			},
		},
		rules: {
			// Svelte 5 $props() destructuring requires let, not const
			'prefer-const': 'off',
			// Intentional use of {@html} for syntax highlighting
			'svelte/no-at-html-tags': 'off',
		},
	},
	{
		files: ['scripts/**'],
		rules: {
			'no-console': 'off',
		},
	},
	{
		ignores: [
			'build/',
			'.svelte-kit/',
			'node_modules/',
			'src-tauri/target/',
		],
	},
);
