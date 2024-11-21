import { forwardRef } from 'react'
import { HiOutlineInformationCircle } from 'react-icons/hi'

import { IconButton, Popover as ChakraPopover, Portal } from '@chakra-ui/react'

export interface ToggleTipProps extends ChakraPopover.RootProps {
  showArrow?: boolean
  portalled?: boolean
  portalRef?: React.RefObject<HTMLElement>
  content?: React.ReactNode
}

export const ToggleTip = forwardRef<HTMLDivElement, ToggleTipProps>(
  (props, ref) => {
    const {
      showArrow,
      children,
      portalled = true,
      content,
      portalRef,
      ...rest
    } = props

    return (
      <ChakraPopover.Root
        {...rest}
        positioning={{ ...rest.positioning, gutter: 4 }}
      >
        <ChakraPopover.Trigger asChild>{children}</ChakraPopover.Trigger>
        <Portal disabled={!portalled} container={portalRef}>
          <ChakraPopover.Positioner>
            <ChakraPopover.Content
              width='auto'
              px='2'
              py='1'
              textStyle='xs'
              rounded='sm'
              ref={ref}
            >
              {showArrow && (
                <ChakraPopover.Arrow>
                  <ChakraPopover.ArrowTip />
                </ChakraPopover.Arrow>
              )}
              {content}
            </ChakraPopover.Content>
          </ChakraPopover.Positioner>
        </Portal>
      </ChakraPopover.Root>
    )
  },
)

export const InfoTip = (props: Partial<ToggleTipProps>) => {
  const { children, ...rest } = props

  return (
    <ToggleTip content={children} {...rest}>
      <IconButton variant='ghost' aria-label='info' size='2xs'>
        <HiOutlineInformationCircle />
      </IconButton>
    </ToggleTip>
  )
}
