import type { RefAttributes, RefObject } from 'react'

import { Menu as ChakraMenu, Portal } from '@chakra-ui/react'

interface MenuContentProps
  extends ChakraMenu.ContentProps,
    RefAttributes<HTMLDivElement> {
  portalled?: boolean
  portalRef?: RefObject<HTMLElement>
}

export function MenuContent({
  portalled = true,
  portalRef,
  ref,
  ...props
}: MenuContentProps) {
  return (
    <Portal disabled={!portalled} container={portalRef}>
      <ChakraMenu.Positioner>
        <ChakraMenu.Content ref={ref} {...props} />
      </ChakraMenu.Positioner>
    </Portal>
  )
}

export const MenuRoot = ChakraMenu.Root
export const MenuItem = ChakraMenu.Item
export const MenuTrigger = ChakraMenu.Trigger
export const MenuTriggerItem = ChakraMenu.TriggerItem
export const MenuSeparator = ChakraMenu.Separator
