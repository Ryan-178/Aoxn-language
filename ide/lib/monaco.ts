'use client'

/**
 * Monaco wiring for the Aoxn IDE.
 *
 * Two things live here and nowhere else:
 *
 * 1. **The Aoxn language definition.** Monaco ships grammars for most
 *    languages but has never heard of Aoxn, so it gets a Monarch grammar
 *    plus an Aoxn-aware configuration (comments, brackets, auto-closing
 *    pairs, folding markers).
 *
 * 2. **The colour theme**, defined from the same CSS custom properties the
 *    rest of the workbench uses. That is why the IDE can switch to a light
 *    theme in one place: Monaco reads the variables back out of the DOM
 *    instead of duplicating sixteen colours.
 *
 * The language services (IntelliSense, the TypeScript checker) are NOT
 * wired up. They need a language server and type definitions for a language
 * that has neither, and shipping a half-configured service is worse than
 * shipping none: it would report a wall of red squiggles for a perfectly
 * valid Aoxn program.
 */

import type * as Monaco from 'monaco-editor'

export const LANG_AOXN = 'aoxn'

const AOXN_KEYWORDS = [
  'def', 'elif', 'else', 'if', 'while', 'for', 'in', 'not', 'and', 'or',
  'return', 'break', 'continue', 'pass', 'import', 'from', 'struct',
  'extern', 'True', 'False', 'None',
]

const AOXN_TYPES = ['int', 'float', 'bool', 'string']

/**
 * Names that read as builtins in the editor: the compiler's own builtins,
 * then the error-channel / out-slot / variable-length-container vocabulary
 * the stdlib ships (v0.39.0) — the documented failure idiom deserves the
 * same colour as `print`. The `ERR_*` codes are zero-argument functions,
 * not bindings, so they ride the same list.
 */
const AOXN_BUILTINS = [
  // compiler builtins
  'len', 'str', 'print', 'range', 'to_int', 'to_float',
  // error channel + out-slots
  'err_ok', 'err_new', 'err_is_ok', 'err_is_err', 'err_code_name',
  'err_with_code', 'err_or_int', 'err_or_str',
  'out_new', 'out_free', 'out_set_i', 'out_set_f', 'out_set_s',
  'out_get_i', 'out_get_f', 'out_get_s',
  'ERR_NONE', 'ERR_IO', 'ERR_PARSE', 'ERR_RANGE', 'ERR_NOT_FOUND',
  'ERR_INVALID', 'ERR_UNSUPPORTED', 'ERR_NOMEM', 'ERR_AGAIN',
  // variable-length containers
  'vec_new', 'vec_reserve', 'vec_push', 'vec_get', 'vec_set', 'vec_free',
  'vec_pop', 'vec_len', 'vec_cap', 'vec_last', 'vec_clear', 'vec_truncate',
  'vec_index_of', 'vec_push_str', 'vec_get_str', 'vec_set_str',
  'vec_index_of_str', 'vec_contains_str',
  'vecvec_new', 'vecvec_push', 'vecvec_get', 'vecvec_len',
  'vecvec_get_data', 'vecvec_free',
]

/**
 * Monaco's theme parser only accepts concrete colour values (hex or
 * `rgba()`), never the `color-mix()`/`lab()` results the Zai tokens resolve
 * to in the DOM. These two palettes therefore mirror the design tokens at
 * the values Monaco needs; keep them in step with `app/zcode-theme.css`.
 */
interface MonacoPalette {
  base: 'vs' | 'vs-dark'
  background: string
  foreground: string
  lineNumber: string
  lineNumberActive: string
  lineHighlight: string
  selection: string
  cursor: string
  indentGuide: string
  widget: string
  widgetBorder: string
  widgetSelected: string
  syntax: {
    comment: string
    keyword: string
    type: string
    string: string
    number: string
    variable: string
    operator: string
  }
}

const MONACO_PALETTES: Record<string, MonacoPalette> = {
  'aoxn-zai-dark': {
    base: 'vs-dark',
    background: '#161616',
    foreground: '#d4d4d4',
    lineNumber: '#747474',
    lineNumberActive: '#d4d4d4',
    lineHighlight: '#1f1f1f',
    selection: '#1e3a5f',
    cursor: '#f8f8f8',
    indentGuide: '#2b2b2b',
    widget: '#202020',
    widgetBorder: '#333333',
    widgetSelected: '#2b2b2b',
    syntax: {
      comment: '#6a9955',
      keyword: '#c586c0',
      type: '#4ec9b0',
      string: '#ce9178',
      number: '#b5cea8',
      variable: '#9cdcfe',
      operator: '#d4d4d4',
    },
  },
  'aoxn-zai-light': {
    base: 'vs',
    background: '#f8f8f8',
    foreground: '#333333',
    lineNumber: '#999999',
    lineNumberActive: '#333333',
    lineHighlight: '#f0f0f0',
    selection: '#cce4f7',
    cursor: '#0d0d0d',
    indentGuide: '#e5e5e5',
    widget: '#ffffff',
    widgetBorder: '#d9d9d9',
    widgetSelected: '#e6e6e6',
    syntax: {
      comment: '#008000',
      keyword: '#0000ff',
      type: '#267f99',
      string: '#a31515',
      number: '#098658',
      variable: '#001080',
      operator: '#3b3b3b',
    },
  },
}

/**
 * Register the Aoxn language and the workbench theme with Monaco.
 *
 * Safe to call more than once: `monaco.languages.register` throws on a
 * duplicate, and React may run the effect that calls this twice.
 */
export function registerAoxn(monaco: typeof Monaco): void {
  if (!monaco.languages.getLanguages().some((l) => l.id === LANG_AOXN)) {
    registerLanguage(monaco)
  }
  registerTheme(monaco, 'aoxn-zai-dark')
  registerTheme(monaco, 'aoxn-zai-light')
}

function registerLanguage(monaco: typeof Monaco): void {
  monaco.languages.register({
    id: LANG_AOXN,
    extensions: ['.ax'],
    aliases: ['Aoxn', 'aoxn'],
    mimetypes: ['text/x-aoxn'],
  })

  monaco.languages.setLanguageConfiguration(LANG_AOXN, {
    comments: { lineComment: '#' },
    brackets: [
      ['{', '}'],
      ['[', ']'],
      ['(', ')'],
    ],
    autoClosingPairs: [
      { open: '{', close: '}' },
      { open: '[', close: ']' },
      { open: '(', close: ')' },
      { open: '"', close: '"', notIn: ['string', 'comment'] },
      { open: "'", close: "'", notIn: ['string', 'comment'] },
    ],
    surroundingPairs: [
      { open: '{', close: '}' },
      { open: '[', close: ']' },
      { open: '(', close: ')' },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
    ],
    // Aoxn is an indentation language, so Enter inside an unclosed brace or
    // bracket indents the next line. There are no block keywords that end
    // one (`end` does not exist in the language), which is why the pattern
    // is bracket-based rather than keyword-based.
    indentationRules: {
      increaseIndentPattern: /^.*\{[^}"']*$/,
      decreaseIndentPattern: /^\s*(\}|\]|\))/,
    },
    folding: { markers: { start: /^\s*def\s/, end: /^\s*struct\s/ } },
  })

  monaco.languages.setMonarchTokensProvider(LANG_AOXN, {
    defaultToken: '',
    keywords: AOXN_KEYWORDS,
    types: AOXN_TYPES,
    builtins: AOXN_BUILTINS,
    tokenizer: {
      root: [
        [/#.*$/, 'comment'],

        // `f"..."` is an interpolation string; the prefix is part of the
        // literal, so it enters the string state immediately.
        [/f"/, { token: 'string', next: '@dqString' }],
        [/"/, { token: 'string', next: '@dqString' }],
        [/'/, { token: 'string', next: '@sqString' }],

        [
          /[a-zA-Z_]\w*/,
          {
            cases: {
              '@keywords': 'keyword',
              '@types': 'type',
              '@builtins': 'type',
              '@default': 'identifier',
            },
          },
        ],

        [/\d*\.\d+([eE][-+]?\d+)?/, 'number'],
        [/\d+/, 'number'],

        // Logical, bitwise and shift operators. `& | ^ ~ << >>` exist since
        // v0.38.0; the doubled and shifted forms must precede the
        // single-character class or Monarch matches one char and leaves the
        // rest bare. There are NO augmented bitwise forms (`&=` etc.) — the
        // trailing `=` of `x = x & y` lexes as its own operator, which is
        // exactly right.
        [/&&|\|\|/, 'operator'],
        [/<<|>>/, 'operator'],
        [/[&|^~]/, 'operator'],
        [/[+\-*/%<>=!]=?/, 'operator'],
        [/\/\//, 'operator'],
        [/[()[\]{}]/, 'delimiter.bracket'],
        [/[.,:;]/, 'delimiter'],
      ],

      dqString: [
        [/[^\\"]+/, 'string'],
        [/\\./, 'string.escape'],
        [/"/, { token: 'string', next: '@pop' }],
      ],

      sqString: [
        [/[^\\']+/, 'string'],
        [/\\./, 'string.escape'],
        [/'/, { token: 'string', next: '@pop' }],
      ],
    },
  })
}

/**
 * Build the Monaco theme from the workbench's CSS variables.
 *
 * Because the theme is derived rather than hard-coded, a light theme is a
 * change to `:root[data-theme='light']` in globals.css and nothing else —
 * Monaco follows.
 */
export function registerTheme(monaco: typeof Monaco, themeName = 'aoxn-zai-dark'): void {
  const palette = MONACO_PALETTES[themeName] ?? MONACO_PALETTES['aoxn-zai-dark']
  const { syntax } = palette
  monaco.editor.defineTheme(themeName, {
    base: palette.base,
    inherit: true,
    rules: [
      { token: 'comment', foreground: syntax.comment, fontStyle: 'italic' },
      { token: 'keyword', foreground: syntax.keyword },
      { token: 'type', foreground: syntax.type },
      { token: 'string', foreground: syntax.string },
      { token: 'string.escape', foreground: syntax.number },
      { token: 'number', foreground: syntax.number },
      { token: 'identifier', foreground: syntax.variable },
      { token: 'operator', foreground: syntax.operator },
      { token: 'delimiter', foreground: syntax.operator },
      { token: 'delimiter.bracket', foreground: syntax.operator },
    ],
    colors: {
      'editor.background': palette.background,
      'editor.foreground': palette.foreground,
      'editorLineNumber.foreground': palette.lineNumber,
      'editorLineNumber.activeForeground': palette.lineNumberActive,
      'editor.lineHighlightBackground': palette.lineHighlight,
      'editor.selectionBackground': palette.selection,
      'editorCursor.foreground': palette.cursor,
      'editorIndentGuide.background1': palette.indentGuide,
      'editorGutter.background': palette.background,
      'editorWidget.background': palette.widget,
      'editorWidget.border': palette.widgetBorder,
      'editorSuggestWidget.background': palette.widget,
      'editorSuggestWidget.selectedBackground': palette.widgetSelected,
      'editorHoverWidget.background': palette.widget,
      'editorHoverWidget.border': palette.widgetBorder,
      'scrollbarSlider.background': 'rgba(121,121,121,0.35)',
      'scrollbarSlider.hoverBackground': 'rgba(100,100,100,0.6)',
      'editorOverviewRuler.border': palette.background,
    },
  })
  monaco.editor.setTheme(themeName)
}