// A React ISLAND: server-rendered Rust (app/server-stats/page.rs) drops this
// into its rsx! tree as `<TodoStats ... />`. The build extracts this props
// interface with oxc and generates typed Rust bindings (crate::client), so a
// mismatched prop is a compile error on the Rust side. No 'use client'
// directive — .tsx IS the client boundary. Styled with the same
// public/style.css classes as the rest of the app.
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
    <section className="detail" data-testid="todo-stats-island">
      <strong>{title ?? 'Todo stats'}</strong>
      {filters.map((f) => (
        <button
          key={f}
          className={filter === f ? 'primary' : 'ghost'}
          onClick={() => setFilter(f)}
        >
          {f}
        </button>
      ))}
      <span
        data-testid="active-count"
        className={`badge ${filter === 'done' ? 'badge-done' : 'badge-open'}`}
      >
        {counts[filter]}
      </span>
    </section>
  );
}
