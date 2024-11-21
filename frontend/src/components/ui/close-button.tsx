import { forwardRef } from 'react'
import { LuX } from 'react-icons/lu'

import type { ButtonProps as ChakraCloseButtonProps } from '@chakra-ui/react'
import { IconButton as ChakraIconButton } from '@chakra-ui/react'

export interface CloseButtonProps extends ChakraCloseButtonProps {}

export const CloseButton = forwardRef<HTMLButtonElement, CloseButtonProps>(
  (props, ref) => {
    return (
      <ChakraIconButton variant='ghost' aria-label='Close' ref={ref} {...props}>
        {props.children ?? <LuX />}
      </ChakraIconButton>
    )
  },
)
