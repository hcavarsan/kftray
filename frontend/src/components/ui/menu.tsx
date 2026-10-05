import type { RefAttributes } from 'react'

import { Menu as ChakraMenu, Portal } from '@chakra-ui/react'

export function MenuContent({
  ref,
  ...props
}: ChakraMenu.ContentProps & RefAttributes<HTMLDivElement>) {
  return (
    <Portal>
      <ChakraMenu.Positioner>
        <ChakraMenu.Content ref={ref} {...props} />
      </ChakraMenu.Positioner>
    </Portal>
  )
}

export const MenuRoot = ChakraMenu.Root
export const MenuItem = ChakraMenu.Item
export const MenuTrigger = ChakraMenu.Trigger
export const MenuSeparator = ChakraMenu.Separator
