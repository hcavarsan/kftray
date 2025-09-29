import { forwardRef } from 'react'
import { X } from 'lucide-react'

import type { ButtonProps as ChakraCloseButtonProps } from '@chakra-ui/react'
import { IconButton as ChakraIconButton } from '@chakra-ui/react'

interface CloseButtonProps extends ChakraCloseButtonProps {}

export const CloseButton = forwardRef<HTMLButtonElement, CloseButtonProps>(
  (props, ref) => {
    return (
      <ChakraIconButton variant='ghost' aria-label='Close' ref={ref} {...props}>
        {props.children ?? <X />}
      </ChakraIconButton>
    )
  },
)
