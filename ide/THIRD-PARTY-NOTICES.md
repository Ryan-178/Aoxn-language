# Third-party notices — Aoxn IDE

The Aoxn IDE is Apache-2.0 (see the repository `LICENSE`). Parts of its
interface are adapted from the ZCode v3.17.2 source archive (`ZCode-main.zip`),
which is Apache-2.0.

## ZCode v3.17.2

Adapted files:

- `app/zcode-theme.css` — the `@theme`, `.dark`, `.theme-zai-light` and
  `.theme-zai-dark` token blocks, copied from
  `packages/ui/src/styles.css`.
- `components/ui/*` — shadcn/Radix primitives from
  `packages/ui/src/components/ui/`. Edits: `.js` import suffixes removed;
  ZCode i18n hooks in `dialog.tsx`, `spinner.tsx` and `kbd.tsx` replaced by
  local strings; `resizable.tsx` accepts an explicit first-run layout.
- `components/ai-elements/conversation.tsx`, `terminal.tsx`,
  `file-tree.tsx` — adapted shells from `packages/ui/src/components/ai-elements/`.
  The `ai` SDK message type in `conversation.tsx` is replaced by a local
  structural type.
- `components/ai-elements/message.tsx` — the ZCode message layout, with a
  `react-markdown` body instead of ZCode's Streamdown pipeline.
- `components/ai-elements/prompt-input.tsx` — the composer layout; the
  attachment/file-part machinery is omitted because the IDE sends text plus
  the active file context.
- `components/lib/utils.ts` — the `cn` helper and its `text-ui-*` merge
  configuration.
- `lib/theme.ts` — ported from `packages/ui/src/useTheme.ts`.

The ZCode files in turn derive some `ai-elements` components from
vercel/ai-elements (Copyright 2023 Vercel, Inc., Apache-2.0); the header in
`components/ai-elements/message.tsx` retains that notice.

## Other runtime dependencies

React, Next.js, Monaco, Radix UI, Tailwind CSS, lucide-react, react-markdown,
remark-gfm, ansi-to-react, react-resizable-panels and use-stick-to-bottom
are used under their respective licenses; the exact version ranges are in
`ide/package.json` and pinned by `ide/pnpm-lock.yaml`.
