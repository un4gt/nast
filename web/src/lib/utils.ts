import { clsx, type ClassValue } from "clsx"
import { extendTailwindMerge } from "tailwind-merge"

// Match the semantic control sizes in tailwind.config.js so layout overrides win.
const twMerge = extendTailwindMerge({
  extend: { theme: { spacing: ['control-sm', 'control-md', 'control-lg'] } },
})

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}
