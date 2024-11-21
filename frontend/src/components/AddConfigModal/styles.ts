import { StylesConfig } from 'react-select'

import { PortOption, StringOption } from '@/types'

export const selectStyles: StylesConfig<StringOption | PortOption> = {
  control: base => ({
    ...base,
    background: '#161616',
    borderColor: 'rgba(255, 255, 255, 0.08)',
    minHeight: '26px',
    height: '30px',
    fontSize: '12px',
    boxShadow: 'none',
    '&:hover': {
      borderColor: 'rgba(255, 255, 255, 0.15)',
    },
  }),
  menu: base => ({
    ...base,
    background: '#161616',
    border: '1px solid rgba(255, 255, 255, 0.08)',
    fontSize: '12px',
    maxHeight: '180px',
  }),
  menuList: base => ({
    ...base,
    maxHeight: '180px',
  }),
  option: (base, state) => ({
    ...base,
    background: state.isFocused ? 'rgba(255, 255, 255, 0.1)' : 'transparent',
    padding: '4px 8px',
    '&:hover': {
      background: 'rgba(255, 255, 255, 0.1)',
    },
  }),
  singleValue: base => ({
    ...base,
    color: 'white',
    fontSize: '12px',
    margin: 0,
    position: 'absolute',
    top: '45%',
    transform: 'translateY(-50%)',
  }),
  input: base => ({
    ...base,
    color: 'white',
    fontSize: '10px',
    margin: 0,
    padding: 0,
  }),
  valueContainer: base => ({
    ...base,
    padding: '0 8px',
    height: '30px',
  }),
  placeholder: base => ({
    ...base,
    color: 'rgba(255, 255, 255, 0.5)',
    fontSize: '12px',
    margin: 0,
    position: 'absolute',
    top: '40%',
    transform: 'translateY(-50%)',
  }),
  indicatorsContainer: base => ({
    ...base,
    height: '30px',
  }),
  dropdownIndicator: base => ({
    ...base,
    padding: '0 4px',
  }),
  clearIndicator: base => ({
    ...base,
    padding: '0 4px',
  }),
}
