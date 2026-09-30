import { HStack, Text } from '@chakra-ui/react'

import { Button } from '@/components/ui/button'
import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { useKubeconfigPicker } from '@/hooks/useKubeconfigPicker'
import { errorMessage } from '@/lib/errors'

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
    onError: error =>
      toaster.error({
        description: errorMessage(error),
        title: 'Error selecting kubeconfig',
      }),
  })

  return (
    <HStack gap={2}>
      <Text color='gray.400' fontSize='2xs'>
        Kubeconfig:
      </Text>
      <Tooltip content={kubeconfig}>
        <Button
          bg='app.hover'
          height='20px'
          onClick={browse}
          px={2}
          size='xs'
          variant='ghost'
          _hover={{ bg: 'app.active' }}
        >
          <Text fontSize='2xs' maxW='120px' truncate>
            {kubeconfig}
          </Text>
        </Button>
      </Tooltip>
    </HStack>
  )
}
