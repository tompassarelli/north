---
name: astro
description: Build, upgrade or debug Astro sites, covering .astro pages, components and layouts, routing, content collections, the Fonts API, images, islands, astro.config, the astro CLI, static builds, adapters and deployment.
grounded: 2026-10-19
written: 2026-10-19
---

# Astro

Written against Astro 7.3/7.4 (Vite 8, Rust compiler, Zod 4); confirm with [docs.astro.build](https://docs.astro.build) and its [llms.txt](https://docs.astro.build/llms.txt) for anything below 7 or above 7.4.

1. Check the installed version (`bun astro --version`) and pin exact versions in `package.json`, choosing the newest release the installer's release-age floor admits, because Astro ships minors weekly and APIs below differ by major.
2. Start a project with `bun create astro@latest`, or hand-write `package.json`, `astro.config.ts`, `tsconfig.json` extending `astro/tsconfigs/strict` and `src/pages/`; run scripts through `bun run`, which supplies the Node shim the `astro` binary needs. Astro 7 requires Node 22.22+ when run under Node.
3. Configure in `astro.config.ts` with `defineConfig` from `astro/config`; set `site` to the deployed origin so canonical URLs and sitemaps resolve. `.cjs`/`.cts` configs no longer load.
4. Route by file under `src/pages/` (`about.astro` → `/about/`, `[slug].astro` with `getStaticPaths()` for dynamic static routes). `src/fetch.ts` and `src/middleware.ts` are reserved entrypoints; leave them absent unless you need the request pipeline.
5. Define content in `src/content.config.ts` with a `loader` on every collection (`glob()` or `file()` from `astro/loaders`) and import `z` from `astro/zod`. Query with `getCollection()`/`getEntry()`, address entries by `id`, and render with `render(entry)` from `astro:content`, because v6 removed `type`, `slug`, `entry.render()` and `astro:content`'s `z`.
6. Use Zod 4 forms in schemas: `z.email()`, `z.url()`, `error` instead of `message`, and `.prefault()` where a default must pass through a transform.
7. Write well-formed HTML, because the Rust compiler errors on unclosed tags and no longer repairs invalid nesting such as `<div>` inside `<p>`. `compressHTML` defaults to `'jsx'`, so add `{" "}` where inline elements need a space.
8. Load fonts through the top-level `fonts` config (`fontProviders.fontsource()`, `.google()` or `.local()`, each with a `cssVariable`) and `<Font cssVariable="..." preload />` from `astro:assets` in `<head>`; use `<Image />`/`<Picture />` from `astro:assets` for `src/` images and `public/` for files served verbatim.
9. Ship zero JS by default; add a framework island with `bun astro add <react|svelte|vue|solid-js|preact>` and a `client:*` directive only where interaction needs it, and `<ClientRouter />` from `astro:transitions` for view transitions.
10. Stay static (`output: 'static'`) unless a route needs request-time rendering; then add an adapter (`bun astro add cloudflare|node|netlify|vercel`) and mark only those routes `export const prerender = false`.
11. Verify with `bun astro check` (needs `@astrojs/check` and `typescript` 5 or 6), then `bun astro build`, then a test that reads `dist/` for the routes, canonical URLs and internal links you promise, because a green build does not prove the pages say the right thing.

Read [notes](references/notes.md) when upgrading from Astro 5 or 6, writing Markdown with remark/rehype plugins, adding an adapter or on-demand rendering, or when a removed API appears in old code or examples.
