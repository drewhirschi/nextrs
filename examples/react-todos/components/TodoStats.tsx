// A React ISLAND: server-rendered Rust (app/server-stats/page.rs) drops this
// into its rsx! tree as `<TodoStats ... />`. The build extracts this props
// interface with oxc and generates typed Rust bindings (crate::client), so a
// mismatched prop is a compile error on the Rust side. No 'use client'
// directive — .tsx IS the client boundary.
import { useState } from 'react';

export interface TodoStatsProps {
  initialFilter: 'all' | 'open' | 'done';
  counts: { all: number; open: number; done: number };
  title?: string;
}

export default function TodoStats({ initialFilter, counts, title }: TodoStatsProps) {
  const [filter, setFilter] = useState(initialFilter);
  const filters = ['all', 'open', 'done'] as const;
  return (
    <section
      data-testid="todo-stats-island"
      style={{ border: '1px solid #ccc', borderRadius: 8, padding: '0.75rem 1rem' }}
    >
      <strong>{title ?? 'Todo stats'}</strong>
      <div style={{ display: 'flex', gap: '0.5rem', marginTop: '0.5rem' }}>
        {filters.map((f) => (
          <button
            key={f}
            onClick={() => setFilter(f)}
            style={{ fontWeight: filter === f ? 'bold' : 'normal' }}
          >
            {f}
          </button>
        ))}
      </div>
      <p data-testid="active-count">
        {filter}: {counts[filter]}
      </p>
    </section>
  );
}
