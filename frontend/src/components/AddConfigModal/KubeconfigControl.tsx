import { HStack, Text } from '@chakra-ui/react'

import { Button } from '@/components/ui/button'
import { Tooltip } from '@/components/ui/tooltip'
import {
  DEFAULT_KUBECONFIG,
  useKubeconfigPicker,
} from '@/hooks/useKubeconfigPicker'

interface KubeconfigControlProps {
  kubeconfig: string
  onChange: (kubeconfig: string) => void
}

export function KubeconfigControl({
  kubeconfig,
  onChange,
}: KubeconfigControlProps) {
  const { browse } = useKubeconfigPicker({
    value: kubeconfig,
    onChange,
    onError: () => onChange(DEFAULT_KUBECONFIG),
    meta: { errorToast: { title: 'Error selecting kubeconfig' } },
  })

  return (
    <HStack gap={2}>
      <Text color='fg.muted' fontSize='2xs'>
        Kubeconfig:
      </Text>
      <Tooltip content={kubeconfig}>
        <Button
          bg='bg.hover'
          height='20px'
          onClick={browse}
          px={2}
          size='xs'
          variant='ghost'
          _hover={{ bg: 'bg.active' }}
        >
          <Text fontSize='2xs' maxW='120px' truncate>
            {kubeconfig}
          </Text>
        </Button>
      </Tooltip>
    </HStack>
  )
}
