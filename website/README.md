# Croft documentation site

[Starlight](https://starlight.astro.build) site, published to GitHub Pages by `.github/workflows/website.yml`.

```sh
npm install
npm run dev       # http://localhost:4321/croft/
npm run build     # static site in dist/
```

Pages are Markdown in `src/content/docs/`; each folder is a sidebar group, ordered by `sidebar.order` in the front matter.
