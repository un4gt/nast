import {
  Empty, EmptyDescription, EmptyHeader, EmptyTitle,
} from '@/components/ui/empty';

export function PlaceholderPanel({ title }: { title: string }) {
  return (
    <Empty>
      <EmptyHeader>
        <EmptyTitle>{title}</EmptyTitle>
        <EmptyDescription>即将支持</EmptyDescription>
      </EmptyHeader>
    </Empty>
  );
}
