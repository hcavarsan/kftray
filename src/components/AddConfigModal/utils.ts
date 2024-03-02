import { StylesConfig } from 'react-select'

import { invoke } from '@tauri-apps/api/tauri'

import theme from '../../assets/theme'
import { KubeContext } from '../../types'

export const customStyles: StylesConfig = {
  control: provided => ({
    ...provided,
    background: theme.colors.gray[800],
    borderColor: theme.colors.gray[700],
  }),
  menu: provided => ({
    ...provided,
    background: theme.colors.gray[800],
  }),
  option: (provided, state) => ({
    ...provided,
    color: state.isSelected ? theme.colors.white : theme.colors.gray[300],
    background: state.isSelected ? theme.colors.gray[600] : 'none',
    cursor: 'pointer',
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
  singleValue: provided => ({
    ...provided,
    color: theme.colors.gray[300],
  }),
  input: provided => ({
    ...provided,
    color: theme.colors.gray[300],
  }),
  placeholder: provided => ({
    ...provided,
    color: theme.colors.gray[500],
  }),
}

export const fetchKubeContexts = (): Promise<KubeContext[]> => {
  return invoke('list_kube_contexts')
}
