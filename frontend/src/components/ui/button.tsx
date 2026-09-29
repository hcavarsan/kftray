import type { ReactNode, RefAttributes } from 'react'

import type { ButtonProps as ChakraButtonProps } from '@chakra-ui/react'
import {
  AbsoluteCenter,
  Button as ChakraButton,
  Span,
  Spinner,
} from '@chakra-ui/react'

interface ButtonProps
  extends ChakraButtonProps,
    RefAttributes<HTMLButtonElement> {
  loading?: boolean
  loadingText?: ReactNode
}

export function Button({
  loading,
  disabled,
  loadingText,
  children,
  ref,
  ...props
}: ButtonProps) {
  return (
    <ChakraButton disabled={loading || disabled} ref={ref} {...props}>
      {loading && !loadingText ? (
        <>
          <AbsoluteCenter display='inline-flex'>
            <Spinner size='inherit' color='inherit' />
          </AbsoluteCenter>
          <Span opacity={0}>{children}</Span>
        </>
      ) : loading ? (
        <>
          <Spinner size='inherit' color='inherit' />
          {loadingText}
        </>
      ) : (
        children
      )}
    </ChakraButton>
  )
}
