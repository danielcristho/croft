# Croft documentation site

[Starlight](https://starlight.astro.build) site, deployed by Cloudflare as static assets, set in `wrangler.jsonc`.

```sh
npm install
npm run dev       # http://localhost:4321/
npm run build     # static site in dist/
```

Pages are Markdown in `src/content/docs/`; each folder is a sidebar group, ordered by `sidebar.order` in the front matter.

Cloudflare builds with `npm run build` and deploys with `npx wrangler deploy`, from this directory.
