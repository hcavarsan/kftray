import { forwardRef } from 'react'

import { Popover as ChakraPopover, Portal } from '@chakra-ui/react'

import { CloseButton } from './close-button'

interface PopoverContentProps extends ChakraPopover.ContentProps {
  portalled?: boolean
  portalRef?: React.RefObject<HTMLElement>
}

export const PopoverContent = forwardRef<HTMLDivElement, PopoverContentProps>(
  (props, ref) => {
    const { portalled = true, portalRef, ...rest } = props

    return (
      <Portal disabled={!portalled} container={portalRef}>
        <ChakraPopover.Positioner>
          <ChakraPopover.Content ref={ref} {...rest} />
        </ChakraPopover.Positioner>
      </Portal>
    )
  },
)

export const PopoverArrow = forwardRef<
  HTMLDivElement,
  ChakraPopover.ArrowProps
>((props, ref) => {
  return (
    <ChakraPopover.Arrow {...props} ref={ref}>
      <ChakraPopover.ArrowTip />
    </ChakraPopover.Arrow>
  )
})

export const PopoverCloseTrigger = forwardRef<
  HTMLButtonElement,
  ChakraPopover.CloseTriggerProps
>((props, ref) => {
  return (
    <ChakraPopover.CloseTrigger
      position='absolute'
      top='1'
      insetEnd='1'
      {...props}
      asChild
      ref={ref}
    >
      <CloseButton size='sm' />
    </ChakraPopover.CloseTrigger>
  )
})

export const PopoverTitle = ChakraPopover.Title
export const PopoverDescription = ChakraPopover.Description
export const PopoverFooter = ChakraPopover.Footer
export const PopoverHeader = ChakraPopover.Header
export const PopoverRoot = ChakraPopover.Root
export const PopoverBody = ChakraPopover.Body
export const PopoverTrigger = ChakraPopover.Trigger
