// shadcn/ui Field composition, adapted to this project's Tailwind 3 tokens.
import type { ComponentProps } from 'react';
import { Label } from './label';
import { cn } from '@/lib/utils';

export function FieldGroup({ className, ...props }: ComponentProps<'div'>) {
  return <div data-slot="field-group" className={cn('flex w-full min-w-0 flex-col gap-5', className)} {...props} />;
}
export function Field({ className, orientation = 'vertical', ...props }: ComponentProps<'div'> & { orientation?: 'vertical' | 'horizontal' }) {
  return <div role="group" data-slot="field" data-orientation={orientation} className={cn('group/field flex min-w-0 gap-2 data-[invalid=true]:text-destructive', orientation === 'horizontal' ? 'items-center justify-between gap-4' : 'flex-col', className)} {...props} />;
}
export function FieldLabel({ className, ...props }: ComponentProps<typeof Label>) {
  return <Label data-slot="field-label" className={cn('leading-snug group-data-[disabled=true]/field:opacity-50', className)} {...props} />;
}
export function FieldDescription({ className, ...props }: ComponentProps<'p'>) {
  return <p data-slot="field-description" className={cn('text-xs font-normal leading-relaxed text-muted-foreground', className)} {...props} />;
}
export function FieldContent({ className, ...props }: ComponentProps<'div'>) {
  return <div data-slot="field-content" className={cn('flex min-w-0 flex-1 flex-col gap-1.5', className)} {...props} />;
}
export function FieldSet({ className, ...props }: ComponentProps<'fieldset'>) {
  return <fieldset data-slot="field-set" className={cn('flex min-w-0 flex-col gap-4', className)} {...props} />;
}
export function FieldLegend({ className, ...props }: ComponentProps<'legend'>) {
  return <legend data-slot="field-legend" className={cn('mb-3 text-sm font-medium', className)} {...props} />;
}
