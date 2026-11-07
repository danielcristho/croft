# Croft documentation site

[Starlight](https://starlight.astro.build) site, deployed by [Cloudflare Pages](https://croft-cog.pages.dev).

```sh
npm install
npm run dev       # http://localhost:4321/
npm run build     # static site in dist/
```

Pages are Markdown in `src/content/docs/`; each folder is a sidebar group, ordered by `sidebar.order` in the front matter.

Cloudflare Pages builds with `npm run build` from this directory and serves `dist`.
