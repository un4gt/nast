import * as React from "react"

import { cn } from "@/lib/utils"

const Textarea = React.forwardRef<
  HTMLTextAreaElement,
  React.ComponentProps<"textarea"> & { variant?: 'default' | 'composer' }
>(({ className, variant = 'default', ...props }, ref) => {
  return (
    <textarea
      className={cn(
        "flex w-full text-base placeholder:text-muted-foreground focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50 md:text-sm",
        variant === 'composer' ? 'composer-input' : 'min-h-[60px] rounded-md border border-input bg-transparent px-3 py-2 shadow-sm focus-visible:ring-1 focus-visible:ring-ring',
        className
      )}
      ref={ref}
      {...props}
    />
  )
})
Textarea.displayName = "Textarea"

export { Textarea }
