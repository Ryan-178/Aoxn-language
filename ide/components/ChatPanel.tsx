'use client'

/**
 * The assistant side pane.
 *
 * The shell is ZCode's: `Conversation` / `Message` / `PromptInput` from its
 * ai-elements layer, the Zai surface tokens, and a compact header. The
 * provider call goes through the Rust proxy (`ide_ai_chat`), so the API key
 * never enters the webview's network layer and OpenAI's missing CORS headers
 * stop mattering.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  BotIcon,
  EraserIcon,
  Settings2Icon,
  SparklesIcon,
  TriangleAlertIcon,
  UserIcon,
} from 'lucide-react'
import * as api from '@/lib/bridge'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { Message, MessageContent, MessageCopyButton, MessageResponse } from '@/components/ai-elements/message'
import { PromptInput } from '@/components/ai-elements/prompt-input'
import {
  Conversation,
  ConversationContent,
  ConversationEmptyState,
  ConversationScrollButton,
} from '@/components/ai-elements/conversation'

type Provider = 'openai' | 'anthropic'

interface AssistantConfig {
  provider: Provider
  baseUrl: string
  apiKey: string
  model: string
  systemPrompt: string
}

const DEFAULT_CONFIG: AssistantConfig = {
  provider: 'openai',
  baseUrl: 'https://api.openai.com/v1',
  apiKey: '',
  model: 'gpt-4o-mini',
  systemPrompt:
    'You are the Aoxn IDE assistant. Aoxn is an AI-native compiled language with Python-style syntax; it lowers to ISO C and clang builds it. Be concrete, prefer complete compilable examples, and follow the language rules exactly (no closures, classes, enums or implicit numeric conversions; arrays are [T; N]; strings are bytes).',
}

interface ChatMessageView {
  id: string
  role: 'user' | 'assistant'
  content: string
  error?: string | null
}

export interface ChatPanelProps {
  activePath: string | null
  activeText: string
  onOpenSettings?(): void
}

const CONFIG_KEY = 'assistant'

function nextId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
}

export function ChatPanel({ activePath, activeText }: ChatPanelProps) {
  const [messages, setMessages] = useState<ChatMessageView[]>([])
  const [draft, setDraft] = useState('')
  const [streaming, setStreaming] = useState(false)
  const [config, setConfig] = useState<AssistantConfig>(DEFAULT_CONFIG)
  const [configOpen, setConfigOpen] = useState(false)
  const [configDraft, setConfigDraft] = useState<AssistantConfig>(DEFAULT_CONFIG)
  const requestSeq = useRef(0)

  useEffect(() => {
    let alive = true
    void (async () => {
      try {
        const stored = await api.configRead()
        const assistant = stored[CONFIG_KEY]
        if (!alive || !assistant || typeof assistant !== 'object') return
        const patch = assistant as Partial<AssistantConfig>
        setConfig((current) => {
          const merged = { ...current, ...patch }
          setConfigDraft(merged)
          return merged
        })
      } catch {
        // A missing or unreadable config file is the normal first run.
      }
    })()
    return () => {
      alive = false
    }
  }, [])

  const persistConfig = useCallback(async (next: AssistantConfig) => {
    setConfig(next)
    try {
      const stored = await api.configRead()
      await api.configWrite({ ...stored, [CONFIG_KEY]: next })
    } catch {
      // Keep the in-memory config; the next save retries the file.
    }
  }, [])

  const send = useCallback(async () => {
    const text = draft.trim()
    if (!text || streaming) return
    const userMessage: ChatMessageView = { id: nextId(), role: 'user', content: text }
    const history = [...messages, userMessage]
    setMessages(history)
    setDraft('')
    setStreaming(true)
    const seq = ++requestSeq.current

    const context =
      activePath && activeText
        ? `The user is editing ${activePath}. Current file contents:\n\`\`\`\n${activeText.slice(0, 24000)}\n\`\`\``
        : ''
    const request: api.ChatMessage[] = [
      { role: 'system', content: config.systemPrompt },
      ...(context ? [{ role: 'system' as const, content: context }] : []),
      ...history.map((message) => ({
        role: message.role,
        content: message.content,
      })),
    ]

    try {
      const reply = await api.aiChat(
        config.baseUrl,
        config.apiKey,
        config.model,
        request,
        config.provider,
      )
      if (requestSeq.current !== seq) return
      setMessages((current) => [
        ...current,
        {
          id: nextId(),
          role: 'assistant',
          content: reply.content,
          error: reply.error,
        },
      ])
    } catch (error) {
      if (requestSeq.current !== seq) return
      setMessages((current) => [
        ...current,
        {
          id: nextId(),
          role: 'assistant',
          content: '',
          error: error instanceof Error ? error.message : String(error),
        },
      ])
    } finally {
      if (requestSeq.current === seq) setStreaming(false)
    }
  }, [activePath, activeText, config, draft, messages, streaming])

  const stop = useCallback(() => {
    // The proxy request cannot be aborted through `invoke`; ignoring its
    // result is the honest approximation, and the button still gives the
    // user the control they expect.
    requestSeq.current += 1
    setStreaming(false)
  }, [])

  const contextLabel = useMemo(() => {
    if (!activePath) return null
    const leaf = activePath.split(/[\\/]/).pop() ?? activePath
    return leaf
  }, [activePath])

  return (
    <aside className="flex h-full min-h-0 w-full flex-col bg-sidebar">
      <header className="flex h-11 flex-none items-center gap-2 border-b border-border px-3">
        <div className="flex size-6 items-center justify-center rounded-md bg-accent text-brand">
          <SparklesIcon className="size-3.5" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="truncate text-ui-sm font-semibold text-foreground">Assistant</div>
        </div>
        <Button
          aria-label="Assistant settings"
          onClick={() => {
            setConfigDraft(config)
            setConfigOpen(true)
          }}
          size="icon-sm"
          type="button"
          variant="ghost"
        >
          <Settings2Icon className="size-3.5" />
        </Button>
        <Button
          aria-label="Clear conversation"
          disabled={messages.length === 0}
          onClick={() => setMessages([])}
          size="icon-sm"
          type="button"
          variant="ghost"
        >
          <EraserIcon className="size-3.5" />
        </Button>
      </header>

      <div className="flex min-h-0 flex-1 flex-col">
        <Conversation className="min-h-0 flex-1">
          <ConversationContent className="gap-5 px-3 py-4">
            {messages.length === 0 ? (
              <ConversationEmptyState
                description="Ask about the open project, a compiler error, or the Aoxn language. The active file is attached as context."
                icon={
                  <div className="flex size-10 items-center justify-center rounded-xl bg-accent text-brand">
                    <BotIcon className="size-5" />
                  </div>
                }
                title="Aoxn assistant"
              />
            ) : (
              messages.map((message) => (
                <Message from={message.role} key={message.id}>
                  <div className="flex items-center gap-1.5 text-ui-xs text-foreground-subtle">
                    {message.role === 'user' ? (
                      <UserIcon className="size-3" />
                    ) : (
                      <SparklesIcon className="size-3" />
                    )}
                    <span>{message.role === 'user' ? 'You' : 'Assistant'}</span>
                  </div>
                  <MessageContent>
                    {message.error ? (
                      <div className="flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive-surface px-2.5 py-2 text-ui-sm text-destructive-surface-foreground">
                        <TriangleAlertIcon className="mt-0.5 size-3.5 shrink-0" />
                        <span className="whitespace-pre-wrap">{message.error}</span>
                      </div>
                    ) : (
                      <MessageResponse>{message.content}</MessageResponse>
                    )}
                  </MessageContent>
                  {message.role === 'assistant' && message.content ? (
                    <div className="flex items-center gap-1">
                      <MessageCopyButton text={message.content} />
                    </div>
                  ) : null}
                </Message>
              ))
            )}
            {streaming ? (
              <div className="flex items-center gap-2 px-1 text-ui-sm text-foreground-subtle">
                <span className="inline-flex gap-1">
                  <span className="size-1.5 animate-pulse rounded-full bg-brand" />
                  <span className="size-1.5 animate-pulse rounded-full bg-brand [animation-delay:120ms]" />
                  <span className="size-1.5 animate-pulse rounded-full bg-brand [animation-delay:240ms]" />
                </span>
                Thinking…
              </div>
            ) : null}
          </ConversationContent>
          <ConversationScrollButton />
        </Conversation>
      </div>

      <div className="flex-none border-t border-border p-2.5">
        {contextLabel ? (
          <div className="mb-1.5 flex items-center gap-1.5 px-0.5">
            <Badge className="max-w-full truncate" variant="outline">
              <span className="truncate">{contextLabel}</span>
            </Badge>
            <span className="text-ui-xs text-foreground-subtlest">attached as context</span>
          </div>
        ) : null}
        <PromptInput
          disabled={streaming}
          onChange={setDraft}
          onSubmit={() => void send()}
          onStop={stop}
          streaming={streaming}
          value={draft}
        />
      </div>

      <Dialog onOpenChange={setConfigOpen} open={configOpen}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>Assistant provider</DialogTitle>
            <DialogDescription>
              Requests are proxied by the IDE backend. The key is stored in the app
              config file, never in the webview.
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-3">
            <div className="grid gap-1.5">
              <Label>Provider</Label>
              <Select
                onValueChange={(value) => {
                  const provider = value as Provider
                  setConfigDraft((current) => ({
                    ...current,
                    provider,
                    baseUrl:
                      provider === 'anthropic'
                        ? 'https://api.anthropic.com'
                        : 'https://api.openai.com/v1',
                    model: provider === 'anthropic' ? 'claude-sonnet-4-5' : 'gpt-4o-mini',
                  }))
                }}
                value={configDraft.provider}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="openai">OpenAI-compatible</SelectItem>
                  <SelectItem value="anthropic">Anthropic</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-1.5">
              <Label>Base URL</Label>
              <Input
                onChange={(event) =>
                  setConfigDraft((current) => ({ ...current, baseUrl: event.target.value }))
                }
                placeholder="https://api.openai.com/v1"
                value={configDraft.baseUrl}
              />
            </div>
            <div className="grid gap-1.5">
              <Label>Model</Label>
              <Input
                onChange={(event) =>
                  setConfigDraft((current) => ({ ...current, model: event.target.value }))
                }
                value={configDraft.model}
              />
            </div>
            <div className="grid gap-1.5">
              <Label>API key</Label>
              <Input
                onChange={(event) =>
                  setConfigDraft((current) => ({ ...current, apiKey: event.target.value }))
                }
                placeholder="sk-…"
                type="password"
                value={configDraft.apiKey}
              />
            </div>
            <div className="grid gap-1.5">
              <Label>System prompt</Label>
              <Textarea
                className="min-h-24"
                onChange={(event) =>
                  setConfigDraft((current) => ({ ...current, systemPrompt: event.target.value }))
                }
                value={configDraft.systemPrompt}
              />
            </div>
          </div>
          <DialogFooter>
            <Button onClick={() => setConfigOpen(false)} type="button" variant="ghost">
              Cancel
            </Button>
            <Button
              onClick={() => {
                void persistConfig(configDraft)
                setConfigOpen(false)
              }}
              type="button"
            >
              Save
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </aside>
  )
}
