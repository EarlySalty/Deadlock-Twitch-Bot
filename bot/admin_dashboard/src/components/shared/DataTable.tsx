import type { ReactNode } from 'react';
import { ArrowDownUp } from 'lucide-react';
import { useState } from 'react';
import { motion } from 'framer-motion';

export interface TableColumn<T> {
  key: string;
  title: string;
  sortable?: boolean;
  className?: string;
  render: (row: T) => ReactNode;
  sortValue?: (row: T) => string | number;
}

interface DataTableProps<T> {
  columns: TableColumn<T>[];
  rows: T[];
  rowKey: (row: T, index: number) => string;
  emptyLabel?: string;
  emptyState?: ReactNode;
  initialSortKey?: string;
  initialSortDirection?: 'asc' | 'desc';
  density?: 'comfortable' | 'compact';
}

export function DataTable<T>({
  columns,
  rows,
  rowKey,
  emptyLabel = 'Keine Daten vorhanden.',
  emptyState,
  initialSortKey = '',
  initialSortDirection = 'asc',
  density = 'comfortable',
}: DataTableProps<T>) {
  const [sortKey, setSortKey] = useState<string>(initialSortKey);
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>(initialSortDirection);
  const headerPaddingClass = density === 'compact' ? 'py-2.5' : 'py-3';
  const cellPaddingClass = density === 'compact' ? 'py-2' : 'py-3';

  const sortedRows = [...rows].sort((left, right) => {
    if (!sortKey) {
      return 0;
    }
    const column = columns.find((entry) => entry.key === sortKey);
    if (!column?.sortValue) {
      return 0;
    }
    const leftValue = column.sortValue(left);
    const rightValue = column.sortValue(right);
    const result = leftValue > rightValue ? 1 : leftValue < rightValue ? -1 : 0;
    return sortDirection === 'asc' ? result : -result;
  });

  return (
    <div className="overflow-hidden rounded-[1.5rem] border border-white/10 bg-bg/35">
      <div className="overflow-x-auto">
        <table className="min-w-full divide-y divide-white/10">
          <thead className="bg-white/5">
            <tr>
              {columns.map((column) => (
                <th
                  key={column.key}
                  className={`px-4 ${headerPaddingClass} text-left text-[0.7rem] font-semibold uppercase tracking-[0.2em] text-text-secondary ${column.className ?? ''}`}
                >
                  {column.sortable ? (
                    <button
                      className="inline-flex items-center gap-2 transition-colors hover:text-white"
                      onClick={() => {
                        if (sortKey === column.key) {
                          setSortDirection((current) => (current === 'asc' ? 'desc' : 'asc'));
                        } else {
                          setSortKey(column.key);
                          setSortDirection('asc');
                        }
                      }}
                    >
                      <span>{column.title}</span>
                      <ArrowDownUp className="h-3.5 w-3.5" />
                    </button>
                  ) : (
                    column.title
                  )}
                </th>
              ))}
            </tr>
          </thead>
          <tbody className="divide-y divide-white/6">
            {sortedRows.length ? (
              sortedRows.map((row, index) => (
                <motion.tr
                  key={rowKey(row, index)}
                  initial={{ opacity: 0, y: 6 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ delay: index * 0.02, duration: 0.18 }}
                  className="interactive-surface"
                >
                  {columns.map((column) => (
                    <td key={column.key} className={`px-4 ${cellPaddingClass} align-top text-sm text-white ${column.className ?? ''}`}>
                      {column.render(row)}
                    </td>
                  ))}
                </motion.tr>
              ))
            ) : (
              <tr>
                <td colSpan={columns.length} className="px-4 py-10 text-center text-sm text-text-secondary">
                  {emptyState ?? emptyLabel}
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
