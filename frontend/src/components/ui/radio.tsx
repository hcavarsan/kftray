import type { InputHTMLAttributes, Ref, RefAttributes } from 'react'

import { RadioGroup as ChakraRadioGroup } from '@chakra-ui/react'

interface RadioProps
  extends ChakraRadioGroup.ItemProps,
    RefAttributes<HTMLInputElement> {
  rootRef?: Ref<HTMLDivElement>
  inputProps?: InputHTMLAttributes<HTMLInputElement>
}

export function Radio({
  children,
  inputProps,
  rootRef,
  ref,
  ...props
}: RadioProps) {
  return (
    <ChakraRadioGroup.Item ref={rootRef} {...props}>
      <ChakraRadioGroup.ItemHiddenInput ref={ref} {...inputProps} />
      <ChakraRadioGroup.ItemIndicator />
      {children && (
        <ChakraRadioGroup.ItemText>{children}</ChakraRadioGroup.ItemText>
      )}
    </ChakraRadioGroup.Item>
  )
}

export const RadioGroup = ChakraRadioGroup.Root
