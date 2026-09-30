import { type MutationMeta, useMutation } from '@tanstack/react-query'

import { selectFile } from '@/lib/nativeDialog'

export const DEFAULT_KUBECONFIG = 'default'

interface KubeconfigPickerOptions {
  value: string
  onChange: (path: string) => void
  meta: MutationMeta
  onError?: () => void
}

export function useKubeconfigPicker({
  value,
  onChange,
  meta,
  onError,
}: KubeconfigPickerOptions) {
  const { mutate: browse } = useMutation({
    mutationFn: selectFile,
    onSuccess: path => {
      if (path) {
        onChange(path)
      }
    },
    onError,
    meta,
  })

  return {
    browse: () => browse(),
    isDefault: value === DEFAULT_KUBECONFIG,
    resetToDefault: () => onChange(DEFAULT_KUBECONFIG),
  }
}
