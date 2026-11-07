'use client'

/*
 * Message primitives, adapted from ZCode's
 * `packages/ui/src/components/ai-elements/message.tsx` (v3.17.2,
 * Apache-2.0). The layout classes are ZCode's; the markdown renderer is
 * `react-markdown` instead of ZCode's full Streamdown pipeline, because the
 * IDE assistant only needs GFM.
 */

import { memo, useState, type ComponentProps, type HTMLAttributes } from 'react'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { CheckIcon, CopyIcon } from 'lucide-react'
import { cn } from '@/components/lib/utils'
import { Button } from '@/components/ui/button'

export type MessageRole = 'user' | 'assistant'

export type MessageProps = HTMLAttributes<HTMLDivElement> & {
  from: MessageRole
}

export const Message = ({ className, from, ...props }: MessageProps) => (
  <div
    className={cn(
      'group flex w-full flex-col gap-2',
      from === 'user' ? 'is-user ml-auto justify-end' : 'is-assistant',
      className,
    )}
    {...props}
  />
)

export type MessageContentProps = HTMLAttributes<HTMLDivElement>

export const MessageContent = ({ children, className, ...props }: MessageContentProps) => (
  <div
    className={cn(
      'is-user:dark flex w-fit min-w-0 max-w-full flex-col gap-2 overflow-hidden text-ui-base',
      'group-[.is-user]:ml-auto group-[.is-user]:rounded-xl group-[.is-user]:bg-user-message-bubble group-[.is-user]:px-3.5 group-[.is-user]:py-2.5 group-[.is-user]:text-foreground',
      'group-[.is-assistant]:w-full group-[.is-assistant]:text-foreground',
      className,
    )}
    {...props}
  >
    {children}
  </div>
)

export type MessageResponseProps = {
  className?: string
  children: string
}

/**
 * Markdown body. Code fences stay plain `<pre>` blocks: the IDE is not a
 * chat product, and pulling Shiki into every message would add megabytes for
 * a feature the editor already provides.
 */
export const MessageResponse = memo(function MessageResponse({
  className,
  children,
}: MessageResponseProps) {
  return (
    <div className={cn('aoxn-markdown', className)}>
      <Markdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children: linkChildren, ...props }) => (
            <a href={href} rel="noreferrer" target="_blank" {...props}>
              {linkChildren}
            </a>
          ),
        }}
      >
        {children}
      </Markdown>
    </div>
  )
})

export type MessageActionsProps = ComponentProps<'div'>

export const MessageActions = ({ className, children, ...props }: MessageActionsProps) => (
  <div className={cn('flex items-center gap-1', className)} {...props}>
    {children}
  </div>
)

export type MessageActionProps = ComponentProps<typeof Button> & {
  label?: string
}

export const MessageAction = ({
  children,
  label,
  variant = 'ghost',
  size = 'icon-sm',
  ...props
}: MessageActionProps) => (
  <Button size={size} type="button" variant={variant} {...props}>
    {children}
    <span className="sr-only">{label}</span>
  </Button>
)

/** Copy a finished assistant answer without selecting it by hand. */
export function MessageCopyButton({ text, className }: { text: string; className?: string }) {
  const [copied, setCopied] = useState(false)
  return (
    <MessageAction
      aria-label="Copy message"
      className={className}
      label="Copy message"
      onClick={() => {
        void navigator.clipboard.writeText(text).then(() => {
          setCopied(true)
          window.setTimeout(() => setCopied(false), 1400)
        })
      }}
    >
      {copied ? <CheckIcon className="size-3.5" /> : <CopyIcon className="size-3.5" />}
    </MessageAction>
  )
}
