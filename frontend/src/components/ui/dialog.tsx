import type { ComponentProps, ReactNode } from 'react'

import { Dialog, Flex, Text } from '@chakra-ui/react'

import { Button } from './button'
import { CloseButton } from './close-button'

interface AppDialogProps {
  title: ReactNode
  onClose: () => void
  children: ReactNode
  headerExtra?: ReactNode
  maxWidth?: string
  height?: string
  closeDisabled?: boolean
  role?: ComponentProps<typeof Dialog.Root>['role']
  placement?: ComponentProps<typeof Dialog.Root>['placement']
  contentProps?: ComponentProps<typeof Dialog.Content>
}

export function AppDialog({
  title,
  onClose,
  children,
  headerExtra,
  maxWidth = '600px',
  height,
  closeDisabled = false,
  role,
  placement,
  contentProps,
}: AppDialogProps) {
  return (
    <Dialog.Root
      open
      role={role}
      placement={placement}
      onOpenChange={({ open }) => !open && !closeDisabled && onClose()}
    >
      <Dialog.Backdrop
        bg='transparent'
        backdropFilter='blur(4px)'
        height='100vh'
      />
      <Dialog.Positioner overflow='hidden'>
        <Dialog.Content
          maxWidth={maxWidth}
          width='90vw'
          height={height}
          bg='app.bg'
          border='1px solid'
          borderColor='app.border'
          borderRadius='lg'
          overflow='hidden'
          position='absolute'
          my={2}
          display='flex'
          flexDirection='column'
          {...contentProps}
        >
          <Dialog.Header
            p={3}
            bg='app.panel'
            borderBottom='1px solid'
            borderColor='app.hover'
            flexShrink={0}
          >
            <Flex align='center' gap={2} width='100%'>
              <Dialog.Title
                flex='1'
                fontSize='sm'
                fontWeight='medium'
                color='gray.100'
              >
                {title}
              </Dialog.Title>
              {headerExtra}
              <Dialog.CloseTrigger position='static' asChild>
                <CloseButton size='2xs' />
              </Dialog.CloseTrigger>
            </Flex>
          </Dialog.Header>

          {children}
        </Dialog.Content>
      </Dialog.Positioner>
    </Dialog.Root>
  )
}

interface ConfirmDialogProps {
  title: ReactNode
  description: ReactNode
  confirmLabel?: string
  isPending: boolean
  onConfirm: () => void
  onClose: () => void
}

export function ConfirmDialog({
  title,
  description,
  confirmLabel = 'Delete',
  isPending,
  onConfirm,
  onClose,
}: ConfirmDialogProps) {
  return (
    <AppDialog
      title={title}
      onClose={onClose}
      closeDisabled={isPending}
      role='alertdialog'
      placement='center'
      maxWidth='400px'
    >
      <Dialog.Body p={3}>
        <Text fontSize='xs' color='gray.400'>
          {description}
        </Text>
      </Dialog.Body>
      <Dialog.Footer
        p={3}
        borderTop='1px solid'
        borderColor='app.hover'
        bg='app.panel'
      >
        <Flex justify='flex-end' gap={2} width='100%'>
          <Button
            size='xs'
            variant='ghost'
            onClick={onClose}
            disabled={isPending}
            _hover={{ bg: 'app.hover' }}
            height='28px'
          >
            Cancel
          </Button>
          <Button
            size='xs'
            bg='blue.500'
            _hover={{ bg: 'blue.600' }}
            disabled={isPending}
            onClick={onConfirm}
            height='28px'
          >
            {confirmLabel}
          </Button>
        </Flex>
      </Dialog.Footer>
    </AppDialog>
  )
}
