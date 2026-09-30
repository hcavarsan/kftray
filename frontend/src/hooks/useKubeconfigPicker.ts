import { selectFile } from '@/lib/nativeDialog'

export const DEFAULT_KUBECONFIG = 'default'

interface KubeconfigPickerOptions {
  value: string
  onChange: (path: string) => void
  onError: (error: unknown) => void
}

export function useKubeconfigPicker({
  value,
  onChange,
  onError,
}: KubeconfigPickerOptions) {
  const browse = async () => {
    try {
      const path = await selectFile()

      if (path) {
        onChange(path)
      }
    } catch (error) {
      onError(error)
    }
  }

  return {
    browse,
    isDefault: value === DEFAULT_KUBECONFIG,
    resetToDefault: () => onChange(DEFAULT_KUBECONFIG),
  }
}
