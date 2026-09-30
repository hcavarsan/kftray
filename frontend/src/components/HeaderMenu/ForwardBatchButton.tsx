import type { LucideIcon } from 'lucide-react'
import { Loader2, X } from 'lucide-react'

import { Box, chakra } from '@chakra-ui/react'

import { Button } from '@/components/ui/button'
import { Tooltip } from '@/components/ui/tooltip'

interface ForwardBatchButtonProps {
  icon: LucideIcon
  label: string
  busyLabel: string
  tooltip: string
  isPending: boolean
  disabled: boolean
  onClick: () => void
  onCancel: () => void
}

export function ForwardBatchButton({
  icon,
  label,
  busyLabel,
  tooltip,
  isPending,
  disabled,
  onClick,
  onCancel,
}: ForwardBatchButtonProps) {
  return (
    <>
      <Tooltip content={tooltip}>
        <Button
          size='xs'
          variant='ghost'
          disabled={disabled}
          onClick={onClick}
          _hover={{ bg: isPending ? undefined : 'whiteAlpha.100' }}
          _disabled={{ cursor: 'not-allowed' }}
          height='26px'
          minWidth='90px'
          bg='whiteAlpha.50'
          px={2}
          borderRadius='md'
          border='1px solid'
          borderColor='app.border'
        >
          <Box
            as={isPending ? Loader2 : icon}
            width='12px'
            height='12px'
            marginRight={1.5}
            animation={isPending ? 'spin 1s linear infinite' : undefined}
          />
          <span style={{ fontSize: '11px' }}>
            {isPending ? busyLabel : label}
          </span>
        </Button>
      </Tooltip>

      {isPending && (
        <Tooltip content='Cancel'>
          <chakra.button
            type='button'
            aria-label='Cancel'
            display='inline-flex'
            alignItems='center'
            justifyContent='center'
            padding='6px'
            borderRadius='sm'
            cursor='pointer'
            bg='transparent'
            border='none'
            _hover={{ bg: 'red.700' }}
            onClick={() => onCancel()}
          >
            <Box as={X} width='10px' height='10px' color='red.300' />
          </chakra.button>
        </Tooltip>
      )}
    </>
  )
}
