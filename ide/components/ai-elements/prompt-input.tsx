'use client'

/*
 * Composer adapted from ZCode's `ai-elements/prompt-input.tsx` (v3.17.2,
 * Apache-2.0). The attachment/file-part machinery is intentionally absent:
 * this composer sends text plus the active file as context, and the
 * backend proxy takes plain chat messages.
 */

import { useCallback, type KeyboardEvent } from 'react'
import { ArrowUpIcon, SquareIcon } from 'lucide-react'
import { cn } from '@/components/lib/utils'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'

export interface PromptInputProps {
  value: string
  onChange(value: string): void
  onSubmit(): void
  onStop?(): void
  disabled?: boolean
  streaming?: boolean
  placeholder?: string
  className?: string
}

export function PromptInput({
  value,
  onChange,
  onSubmit,
  onStop,
  disabled = false,
  streaming = false,
  placeholder = 'Ask about this project…',
  className,
}: PromptInputProps) {
  const handleKeyDown = useCallback(
    (event: KeyboardEvent<HTMLTextAreaElement>) => {
      if (event.key !== 'Enter' || event.shiftKey) return
      event.preventDefault()
      if (!disabled && value.trim()) onSubmit()
    },
    [disabled, onSubmit, value],
  )

  return (
    <div
      className={cn(
        'relative flex w-full flex-col gap-2 rounded-xl border border-input-border bg-input p-2 transition-colors focus-within:border-input-border-focused',
        className,
      )}
      data-slot="prompt-input"
    >
      <Textarea
        aria-label="Message the assistant"
        className="max-h-52 min-h-14 resize-none border-0 bg-transparent px-1.5 py-1 shadow-none focus-visible:border-0 focus-visible:ring-0"
        disabled={disabled && !streaming}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={handleKeyDown}
        placeholder={placeholder}
        rows={2}
        value={value}
      />
      <div className="flex items-center justify-between gap-2 px-1">
        <span
          className="truncate whitespace-nowrap text-ui-xs text-foreground-subtlest"
          title="Enter to send · Shift+Enter for a new line"
        >
          Enter to send
        </span>
        {streaming ? (
          <Button
            aria-label="Stop"
            onClick={onStop}
            size="icon-sm"
            type="button"
            variant="outline"
          >
            <SquareIcon className="size-3" />
          </Button>
        ) : (
          <Button
            aria-label="Send"
            disabled={disabled || !value.trim()}
            onClick={onSubmit}
            size="icon-sm"
            type="button"
          >
            <ArrowUpIcon className="size-3.5" />
          </Button>
        )}
      </div>
    </div>
  )
}
