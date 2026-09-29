import type { ReactNode, RefAttributes, RefObject } from 'react'

import { Tooltip as ChakraTooltip, Portal } from '@chakra-ui/react'

interface TooltipProps
  extends ChakraTooltip.RootProps,
    RefAttributes<HTMLDivElement> {
  showArrow?: boolean
  portalled?: boolean
  portalRef?: RefObject<HTMLElement>
  content: ReactNode
  contentProps?: ChakraTooltip.ContentProps
  disabled?: boolean
}

export function Tooltip({
  showArrow,
  children,
  disabled,
  portalled,
  content,
  contentProps,
  portalRef,
  ref,
  ...props
}: TooltipProps) {
  if (disabled) {
    return children
  }
  return (
    <ChakraTooltip.Root {...props}>
      <ChakraTooltip.Trigger asChild>{children}</ChakraTooltip.Trigger>
      <Portal disabled={!portalled} container={portalRef}>
        <ChakraTooltip.Positioner>
          <ChakraTooltip.Content ref={ref} {...contentProps}>
            {showArrow && (
              <ChakraTooltip.Arrow>
                <ChakraTooltip.ArrowTip />
              </ChakraTooltip.Arrow>
            )}
            {content}
          </ChakraTooltip.Content>
        </ChakraTooltip.Positioner>
      </Portal>
    </ChakraTooltip.Root>
  )
}
