import type { InputHTMLAttributes, ReactNode, Ref, RefAttributes } from 'react'

import { Checkbox as ChakraCheckbox } from '@chakra-ui/react'

interface CheckboxProps
  extends ChakraCheckbox.RootProps,
    RefAttributes<HTMLInputElement> {
  icon?: ReactNode
  inputProps?: InputHTMLAttributes<HTMLInputElement>
  rootRef?: Ref<HTMLLabelElement>
}

export function Checkbox({
  icon,
  children,
  inputProps,
  rootRef,
  ref,
  ...props
}: CheckboxProps) {
  return (
    <ChakraCheckbox.Root ref={rootRef} {...props}>
      <ChakraCheckbox.HiddenInput ref={ref} {...inputProps} />
      <ChakraCheckbox.Control>
        {icon || <ChakraCheckbox.Indicator />}
      </ChakraCheckbox.Control>
      {children != null && (
        <ChakraCheckbox.Label>{children}</ChakraCheckbox.Label>
      )}
    </ChakraCheckbox.Root>
  )
}
