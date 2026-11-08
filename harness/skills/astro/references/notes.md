# Astro: version notes

Read this when upgrading a project, porting an older example, or touching
Markdown plugins, adapters or on-demand rendering. Facts were taken from the
official v6 and v7 upgrade guides and the Fonts and Routing guides on
docs.astro.build on 2026-10-19; recheck there for later majors.

## Upgrading

Run `bunx @astrojs/upgrade` and read the upgrade guide for every major crossed,
because each major removes the previous major's deprecations.

## Astro 6 removals and renames

| Old | Now |
| --- | --- |
| `src/content/config.ts` | `src/content.config.ts` (old path throws `LegacyContentConfigError`) |
| collections without `loader`, `type: 'content' \| 'data'` | `loader: glob({ pattern, base })` or `file(path)` from `astro/loaders` |
| `entry.slug`, `getEntryBySlug()`, `getDataEntryById()` | `entry.id`, `getEntry()`; `entry.filePath` for the file |
| `entry.render()` | `render(entry)` from `astro:content` |
| `z` from `astro:content` or `astro:schema` | `z` from `astro/zod` (Zod 4) |
| `Astro.glob()` | `import.meta.glob()` (synchronous result) |
| `<ViewTransitions />` | `<ClientRouter />` from `astro:transitions` |
| `experimental.csp` | `security.csp` |
| `experimental.fonts` | top-level `fonts` |
| `emitESMImage()` | `emitImageMetadata()` |
| `Astro.site` in `getStaticPaths()` | `import.meta.env.SITE` |

Other v6 behaviour: `import.meta.env` values are inlined and never coerced
(compare `=== 'true'`); use `astro:env` or `process.env` for secrets. Images
crop by default and never upscale; `getImage()` throws on the client.
Endpoints with a file extension such as `/sitemap.xml` take no trailing slash.
Heading IDs keep trailing hyphens. Scripts and styles render in source order.

## Astro 7 changes

- Vite 8 for dev and build; plugins using Vite internals may need updates.
- The Rust compiler is the only compiler: unclosed non-void tags are errors,
  invalid nesting is not auto-corrected, and minified CSS may differ
  cosmetically (`rebeccapurple` → `#639`).
- `compressHTML` defaults to `'jsx'`; set `true` for the old behaviour or
  `false` to keep all whitespace.
- Markdown uses the Sätteri processor by default. To keep remark or rehype
  plugins, install `@astrojs/markdown-remark` and set
  `markdown: { processor: unified() }`.
- Formerly experimental and now top level or default: `logger`
  (`logHandlers.json()`), `cache` and `routeRules`, queued rendering and
  advanced routing (`src/fetch.ts`, rename it with `fetchFile` or disable it
  with `fetchFile: null`).
- Removed: `@astrojs/db` (use `node:sqlite`, Drizzle or another library) and
  the `TRANSITION_*` constants, `isTransition*Event()` and
  `createAnimationScope()` from `astro:transitions`; compare
  `event.type === 'astro:after-swap'` instead.
- `getContainerRenderer()` moves to each renderer's `/container-renderer`
  entrypoint.
- 7.1 added CSP options, custom pagination URLs and lower-memory collections;
  7.2 added experimental incremental static builds and background
  `astro preview`.

## Adapters and on-demand rendering

`bun astro add <adapter> --yes` installs the adapter and edits the config.
Official adapters bumped majors with Astro 6 and the Cloudflare adapter changed
substantially; read its integration guide before reusing old Wrangler config.
With an adapter, keep pages prerendered by default and opt individual routes
out with `export const prerender = false`, so static pages stay cacheable.

## Bun

Astro documents Bun as a supported package manager and runtime with the caveat
that some integrations may misbehave. `bun run <script>` executes the
Node-shebang `astro` binary through Bun's Node shim, so a machine without Node
can still build. Add `@types/bun` when tests use `bun:test`.

## Provenance

Adapted from the `astro` skill in astrolicious/agent-skills at
`cb4839f3794189ebf35c55d3ba5d3876dff7acf0`, whose frontmatter declares
`license: MIT` and credits the "Astro Team"; see
this package's `PROVENANCE.md` and `NOTICE`.
