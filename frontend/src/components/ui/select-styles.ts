import type { GroupBase, StylesConfig } from 'react-select'

const token = (name: string) => `var(--chakra-colors-app-${name})`

const centered = {
  margin: 0,
  position: 'absolute',
  top: '45%',
  transform: 'translateY(-50%)',
} as const

export function selectStyles<Option, IsMulti extends boolean = false>(
  height = 30,
): StylesConfig<Option, IsMulti, GroupBase<Option>> {
  return {
    control: (base, state) => ({
      ...base,
      background: token('panel'),
      borderColor: token('border'),
      minHeight: `${height}px`,
      ...(!state.isMulti && { height: `${height}px` }),
      fontSize: '12px',
      boxShadow: 'none',
      '&:hover': { borderColor: token('border-strong') },
    }),
    menu: base => ({
      ...base,
      background: token('panel'),
      border: `1px solid ${token('border')}`,
      fontSize: '12px',
    }),
    menuList: base => ({ ...base, maxHeight: '180px' }),
    option: (base, state) => ({
      ...base,
      background: state.isFocused ? token('active') : 'transparent',
      padding: '4px 8px',
      '&:hover': { background: token('active') },
    }),
    singleValue: base => ({
      ...base,
      ...centered,
      color: 'white',
      fontSize: '12px',
    }),
    input: base => ({
      ...base,
      color: 'white',
      fontSize: '12px',
      margin: 0,
      padding: 0,
    }),
    valueContainer: (base, state) => ({
      ...base,
      padding: state.isMulti ? '2px 8px' : '0 8px',
      gap: '4px',
    }),
    placeholder: (base, state) => ({
      ...base,
      ...(!state.isMulti && centered),
      color: token('placeholder'),
      fontSize: '12px',
    }),
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
      color: 'rgba(147, 197, 253, 1)',
      fontSize: '11px',
      padding: '0 4px',
    }),
    multiValueRemove: base => ({
      ...base,
      color: 'rgba(147, 197, 253, 0.8)',
      ':hover': { background: token('accent-muted'), color: 'white' },
    }),
  }
}
