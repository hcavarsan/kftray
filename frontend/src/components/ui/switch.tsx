import type { InputHTMLAttributes, ReactNode, Ref, RefAttributes } from 'react'

import { Switch as ChakraSwitch } from '@chakra-ui/react'

interface SwitchProps
  extends ChakraSwitch.RootProps,
    RefAttributes<HTMLInputElement> {
  inputProps?: InputHTMLAttributes<HTMLInputElement>
  rootRef?: Ref<HTMLLabelElement>
  trackLabel?: { on: ReactNode; off: ReactNode }
  thumbLabel?: { on: ReactNode; off: ReactNode }
}

export function Switch({
  inputProps,
  children,
  rootRef,
  trackLabel,
  thumbLabel,
  ref,
  ...props
}: SwitchProps) {
  return (
    <ChakraSwitch.Root ref={rootRef} {...props}>
      <ChakraSwitch.HiddenInput ref={ref} {...inputProps} />
      <ChakraSwitch.Control>
        <ChakraSwitch.Thumb>
          {thumbLabel && (
            <ChakraSwitch.ThumbIndicator fallback={thumbLabel.off}>
              {thumbLabel.on}
            </ChakraSwitch.ThumbIndicator>
          )}
        </ChakraSwitch.Thumb>
        {trackLabel && (
          <ChakraSwitch.Indicator fallback={trackLabel.off}>
            {trackLabel.on}
          </ChakraSwitch.Indicator>
        )}
      </ChakraSwitch.Control>
      {children != null && <ChakraSwitch.Label>{children}</ChakraSwitch.Label>}
    </ChakraSwitch.Root>
  )
}
