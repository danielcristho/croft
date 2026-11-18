// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
	site: 'https://croft-docs.pages.dev',
	// Diagrams live in the repository's assets/, shared with the README.
	vite: { server: { fs: { allow: ['..'] } } },
	integrations: [
		starlight({
			title: 'Croft',
			description: 'Autoscaling that reacts to what processes do in the kernel.',
			social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/danielcristho/croft' }],
			editLink: { baseUrl: 'https://github.com/danielcristho/croft/edit/main/website/' },
			customCss: ['./src/styles/custom.css'],
			sidebar: [
				{ label: 'Start here', items: [{ autogenerate: { directory: 'start' } }] },
				{ label: 'Guides', items: [{ autogenerate: { directory: 'guides' } }] },
				{ label: 'Reference', items: [{ autogenerate: { directory: 'reference' } }] },
				{ label: 'Research', items: [{ autogenerate: { directory: 'research' } }] },
				{ label: 'Contributing', items: [{ autogenerate: { directory: 'contributing' } }] },
			],
		}),
	],
});
