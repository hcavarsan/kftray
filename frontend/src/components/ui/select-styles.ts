import type { GroupBase, StylesConfig } from 'react-select'

const token = (name: string) => `var(--chakra-colors-${name})`

const centered = (top: string) =>
  ({
    margin: 0,
    position: 'absolute',
    top,
    transform: 'translateY(-50%)',
  }) as const

export function selectStyles<Option, IsMulti extends boolean = false>(
  height = 30,
): StylesConfig<Option, IsMulti, GroupBase<Option>> {
  return {
    control: (base, state) => ({
      ...base,
      background: token('bg-surface'),
      borderColor: token('border'),
      minHeight: `${height}px`,
      ...(!state.isMulti && { height: `${height}px` }),
      fontSize: '12px',
      boxShadow: 'none',
      '&:hover': { borderColor: token('border-emphasized') },
    }),
    menu: base => ({
      ...base,
      background: token('bg-surface'),
      border: `1px solid ${token('border')}`,
      fontSize: '12px',
    }),
    menuList: base => ({ ...base, maxHeight: '180px' }),
    option: (base, state) => ({
      ...base,
      background: state.isFocused ? token('bg-active') : 'transparent',
      padding: '4px 8px',
      '&:hover': { background: token('bg-active') },
    }),
    singleValue: base => ({
      ...base,
      ...centered('45%'),
      color: token('fg'),
      fontSize: '12px',
    }),
    input: base => ({
      ...base,
      color: token('fg'),
      fontSize: '12px',
      margin: 0,
      padding: 0,
    }),
    valueContainer: (base, state) => ({
      ...base,
      ...(state.isMulti
        ? { padding: '2px 8px', gap: '4px' }
        : { padding: '0 8px', height: `${height}px` }),
    }),
    placeholder: (base, state) => ({
      ...base,
      ...(!state.isMulti && centered('40%')),
      color: token('fg-subtle'),
      fontSize: '12px',
    }),
    indicatorsContainer: (base, state) =>
      state.isMulti ? base : { ...base, height: `${height}px` },
    dropdownIndicator: base => ({ ...base, padding: '0 4px' }),
    clearIndicator: base => ({ ...base, padding: '0 4px' }),
    multiValue: base => ({
      ...base,
      background: token('accent-subtle'),
      border: `1px solid ${token('accent-muted')}`,
      borderRadius: '4px',
      margin: 0,
    }),
    multiValueLabel: base => ({
      ...base,
      color: token('accent-fg'),
      fontSize: '11px',
      padding: '0 4px',
    }),
    multiValueRemove: base => ({
      ...base,
      color: token('accent-fg'),
      ':hover': { background: token('accent-muted'), color: token('fg') },
    }),
  }
}
