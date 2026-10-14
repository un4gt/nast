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
        variant === 'composer' ? 'composer-input' : 'min-h-[60px] rounded-lg border border-input bg-card/70 px-3 py-2 shadow-xs transition-[border-color,box-shadow] focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/15',
        className
      )}
      ref={ref}
      {...props}
    />
  )
})
Textarea.displayName = "Textarea"

export { Textarea }
