import type { ViewCondition } from '@/types'

const FIELD_LABELS: Record<string, string> = {
  context: 'Context',
  namespace: 'Namespace',
  kubeconfig: 'Kubeconfig',
  workload_type: 'Workload type',
  protocol: 'Protocol',
}

export const isTagField = (field: string) => field.startsWith('tag:')

export const fieldLabel = (field: string) =>
  isTagField(field) ? `Tag: ${field.slice(4)}` : (FIELD_LABELS[field] ?? field)

export const toggleFilterValue = (
  filters: ViewCondition[],
  field: string,
  value: string,
): ViewCondition[] => {
  const values = filters.find(c => c.field === field)?.values ?? []
  const nextValues = values.includes(value)
    ? values.filter(v => v !== value)
    : [...values, value]
  const rest = filters.filter(c => c.field !== field)

  return nextValues.length ? [...rest, { field, values: nextValues }] : rest
}

export const toggleFilterAny = (
  filters: ViewCondition[],
  field: string,
): ViewCondition[] => {
  const current = filters.find(c => c.field === field)
  const rest = filters.filter(c => c.field !== field)

  return current?.values.length === 0 ? rest : [...rest, { field, values: [] }]
}
