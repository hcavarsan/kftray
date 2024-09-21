import { StylesConfig } from 'react-select'

import { invoke } from '@tauri-apps/api/tauri'

import theme from '../../assets/theme'
import { KubeContext } from '../../types'

export const customStyles: StylesConfig = {
  control: (provided, state) => ({
    ...provided,
    minHeight: '26px',
    height: '26px',
    fontSize: '0.75rem',
    background: state.isDisabled
      ? theme.colors.gray[900]
      : theme.colors.gray[800],
    borderColor: state.isDisabled
      ? theme.colors.gray[900]
      : theme.colors.gray[700],
    boxShadow: 'none',
    '&:hover': {
      borderColor: theme.colors.gray[600],
    },
  }),
  valueContainer: provided => ({
    ...provided,
    height: '26px',
    padding: '0 4px',
  }),
  input: provided => ({
    ...provided,
    margin: '0px',
    color: theme.colors.gray[600],
  }),
  indicatorsContainer: provided => ({
    ...provided,
    height: '26px',
  }),
  clearIndicator: provided => ({
    ...provided,
    padding: '4px',
  }),
  dropdownIndicator: provided => ({
    ...provided,
    padding: '4px',
  }),
  option: (provided, state) => ({
    ...provided,
    color: state.isSelected ? theme.colors.white : theme.colors.gray[300],
    background: state.isSelected ? theme.colors.gray[600] : 'none',
    cursor: 'pointer',
    fontSize: '0.70rem',
    ':active': {
      ...provided[':active'],
      background: theme.colors.gray[500],
    },
    ':hover': {
      ...provided[':hover'],
      background: theme.colors.gray[700],
      color: theme.colors.white,
    },
  }),
  menu: provided => ({
    ...provided,
    background: theme.colors.gray[800],
    fontSize: '0.70rem',
    maxHeight: '200px',
    overflowY: 'auto',
  }),
  menuList: provided => ({
    ...provided,
    maxHeight: '200px',
    overflowY: 'auto',
  }),
  singleValue: provided => ({
    ...provided,
    color: theme.colors.gray[300],
  }),
  placeholder: provided => ({
    ...provided,
    color: theme.colors.gray[500],
  }),
}

export const fetchKubeContexts = (
  kubeConfig?: string,
): Promise<KubeContext[]> => {
  console.log('fetchKubeContexts', kubeConfig)

  return invoke('list_kube_contexts', { kubeconfig: kubeConfig })
}
