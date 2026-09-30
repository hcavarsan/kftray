import type { RefAttributes } from 'react'
import { X } from 'lucide-react'

import type { ButtonProps as ChakraCloseButtonProps } from '@chakra-ui/react'
import { IconButton as ChakraIconButton } from '@chakra-ui/react'

export function CloseButton({
  children,
  ref,
  ...props
}: ChakraCloseButtonProps & RefAttributes<HTMLButtonElement>) {
  return (
    <ChakraIconButton variant='ghost' aria-label='Close' ref={ref} {...props}>
      {children ?? <X />}
    </ChakraIconButton>
  )
}
