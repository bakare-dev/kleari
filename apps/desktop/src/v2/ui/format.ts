/**
 * Decimal gigabytes as the v2 screens show them, in the unit that keeps a figure readable:
 * "3.2 GB", "640 MB", "48 KB". Something smaller than 1 KB reads "< 1 KB", never "0 MB".
 */
export function formatGb(gb: number): string {
  if (gb >= 0.9995) {
    return `${gb.toFixed(1)} GB`;
  }
  const mb = gb * 1000;
  if (mb >= 0.9995) {
    return `${Math.round(mb)} MB`;
  }
  const kb = mb * 1000;
  if (kb >= 0.5) {
    return `${Math.round(kb)} KB`;
  }
  return gb > 0 ? "< 1 KB" : "0 KB";
}

/** "1 item" / "3 items". */
export function plural(count: number, one: string, many = `${one}s`): string {
  return `${count} ${count === 1 ? one : many}`;
}

export function sum<T>(items: readonly T[], value: (item: T) => number): number {
  return items.reduce((total, item) => total + value(item), 0);
}
