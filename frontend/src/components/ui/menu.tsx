'use client'

import { forwardRef } from 'react'

import { Menu as ChakraMenu, Portal } from '@chakra-ui/react'

interface MenuContentProps extends ChakraMenu.ContentProps {
  portalled?: boolean
  portalRef?: React.RefObject<HTMLElement>
}

export const MenuContent = forwardRef<HTMLDivElement, MenuContentProps>(
  (props, ref) => {
    const { portalled = true, portalRef, ...rest } = props

    return (
      <Portal disabled={!portalled} container={portalRef}>
        <ChakraMenu.Positioner>
          <ChakraMenu.Content ref={ref} {...rest} />
        </ChakraMenu.Positioner>
      </Portal>
    )
  },
)

export const MenuRoot = ChakraMenu.Root
export const MenuItem = ChakraMenu.Item
export const MenuTrigger = ChakraMenu.Trigger
export const MenuTriggerItem = ChakraMenu.TriggerItem
