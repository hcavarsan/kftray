import type { ComponentProps, ReactNode } from 'react'

import { Dialog, Flex, Portal, Text } from '@chakra-ui/react'

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
}: AppDialogProps) {
  return (
    <Dialog.Root
      open
      role={role}
      placement={placement}
      onOpenChange={({ open }) => !open && !closeDisabled && onClose()}
      onFocusOutside={event => event.preventDefault()}
      onEscapeKeyDown={event => {
        if (
          event.target instanceof Element &&
          event.target.closest('[aria-haspopup][aria-expanded="true"]')
        ) {
          event.preventDefault()
        }
      }}
    >
      <Portal>
        <Dialog.Backdrop bg='transparent' backdropFilter='blur(4px)' />
        <Dialog.Positioner overflow='hidden'>
          <Dialog.Content
            maxWidth={maxWidth}
            width='90vw'
            height={height}
            bg='bg.canvas'
            border='1px solid'
            borderColor='border'
            borderRadius='lg'
            boxShadow='dialog'
            overflow='hidden'
            position='absolute'
            my={2}
            display='flex'
            flexDirection='column'
          >
            <Dialog.Header
              px={3}
              py={2}
              bg='bg.surface'
              borderBottom='1px solid'
              borderColor='border.subtle'
              flexShrink={0}
            >
              <Flex align='center' gap={2} width='100%'>
                <Dialog.Title
                  flex='1'
                  minWidth={0}
                  fontSize='sm'
                  lineHeight='20px'
                  fontWeight='medium'
                  color='fg'
                  truncate
                >
                  {title}
                </Dialog.Title>
                {headerExtra}
                <Dialog.CloseTrigger position='static' asChild>
                  <CloseButton size='2xs' disabled={closeDisabled} />
                </Dialog.CloseTrigger>
              </Flex>
            </Dialog.Header>

            {children}
          </Dialog.Content>
        </Dialog.Positioner>
      </Portal>
    </Dialog.Root>
  )
}

export function AppDialogBody(props: ComponentProps<typeof Dialog.Body>) {
  return (
    <Dialog.Body p={3} flex='1' minHeight={0} overflowY='auto' {...props} />
  )
}

export function AppDialogFooter({
  children,
  justify = 'flex-end',
}: {
  children: ReactNode
  justify?: 'flex-end' | 'space-between'
}) {
  return (
    <Dialog.Footer
      px={3}
      py={2}
      bg='bg.surface'
      borderTop='1px solid'
      borderColor='border.subtle'
      flexShrink={0}
    >
      <Flex justify={justify} align='center' gap={2} width='100%'>
        {children}
      </Flex>
    </Dialog.Footer>
  )
}

export function DialogCancelButton({
  label = 'Cancel',
  onClick,
  disabled,
}: {
  label?: string
  onClick: () => void
  disabled?: boolean
}) {
  return (
    <Button
      size='xs'
      variant='ghost'
      height='28px'
      color='fg.muted'
      _hover={{ bg: 'bg.faint' }}
      onClick={onClick}
      disabled={disabled}
    >
      {label}
    </Button>
  )
}

interface ConfirmDialogProps {
  title: ReactNode
  description: ReactNode
  isPending: boolean
  onConfirm: () => void
  onClose: () => void
}

export function ConfirmDialog({
  title,
  description,
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
        <Text fontSize='xs' color='fg.muted'>
          {description}
        </Text>
      </Dialog.Body>
      <AppDialogFooter>
        <DialogCancelButton onClick={onClose} disabled={isPending} />
        <Button
          size='xs'
          height='28px'
          colorPalette='red'
          loading={isPending}
          loadingText='Deleting...'
          onClick={onConfirm}
        >
          Delete
        </Button>
      </AppDialogFooter>
    </AppDialog>
  )
}
