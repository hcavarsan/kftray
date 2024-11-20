'use client'

import { useEffect, useMemo, useRef } from 'react'
import debounce from 'lodash/debounce'
import { MdClose } from 'react-icons/md'

import {
  createToaster,
  Portal,
  Spinner,
  Stack,
  Toast,
  Toaster as ChakraToaster,
} from '@chakra-ui/react'

interface ToastOptions {
  title?: string
  description?: string
  duration?: number
  action?: {
    label: string
    onClick: () => void
  }
  onStatusChange?: (details: { status: string }) => void
}

type ToastFunction = (options: ToastOptions) => string | undefined

const createToastWrapper = (
  originalToaster: ReturnType<typeof createToaster>,
) => {
  const wrapToastFunction =
    (fn: ToastFunction): ToastFunction =>
      (options: ToastOptions) => {
        const id = fn({
          ...options,
          duration: options.duration ?? 1000,
          onStatusChange: (details: { status: string }) => {
            options.onStatusChange?.(details)
          },
        })

        return id
      }

  return {
    ...originalToaster,
    success: wrapToastFunction(originalToaster.success),
    error: wrapToastFunction(originalToaster.error),
    loading: wrapToastFunction(originalToaster.loading),
    create: wrapToastFunction(originalToaster.create),
  }
}

export const toaster = createToastWrapper(
  createToaster({
    placement: 'top-end',
    duration: 1000,
    overlap: true,
    offsets: {
      top: '5px',
      right: '5px',
      bottom: '5px',
      left: '5px',
    },
  }),
)

export const Toaster = () => {
  const toastRef = useRef<HTMLDivElement>(null)

  const debouncedDismiss = useMemo(
    () =>
      debounce((event: MouseEvent) => {
        const target = event.target as HTMLElement
        const toastElement = toastRef.current

        if (toastElement && !toastElement.contains(target)) {
          toaster.dismiss()
        }
      }, 200),
    [],
  )

  useEffect(() => {
    const handleClick = (event: MouseEvent) => {
      if (!toastRef.current) {
        return
      }
      debouncedDismiss(event)
    }

    document.addEventListener('mousedown', handleClick)

    return () => {
      debouncedDismiss.cancel()
      document.removeEventListener('mousedown', handleClick)
    }
  }, [debouncedDismiss])

  return (
    <Portal>
      <ChakraToaster
        toaster={toaster}
        insetInline={{ mdDown: '2' }}
        insetBlock={{ mdDown: '2' }}
      >
        {toast => (
          <Toast.Root
            ref={toastRef}
            width={{ base: '240px', md: '260px' }}
            maxWidth='90%'
            py='2'
            px='3'
            bg='gray.900'
            borderRadius='lg'
            boxShadow='dark-lg'
            border='1px solid'
            borderColor='gray.800'
            onClick={e => e.stopPropagation()}
          >
            {toast.type === 'loading' ? (
              <Spinner size='xs' color='gray.500' />
            ) : (
              <Toast.Indicator />
            )}
            <Stack gap='0' flex='1' maxWidth='100%'>
              {toast.title && (
                <Toast.Title fontSize='xs' fontWeight='normal' color='gray.200'>
                  {toast.title}
                </Toast.Title>
              )}
              {toast.description && (
                <Toast.Description fontSize='xs' color='gray.300'>
                  {toast.description}
                </Toast.Description>
              )}
            </Stack>
            {toast.action && (
              <Toast.ActionTrigger
                fontSize='xs'
                color='gray.300'
                _hover={{ color: 'gray.300' }}
                ml='2'
              >
                {toast.action.label}
              </Toast.ActionTrigger>
            )}
            <Toast.CloseTrigger
              color='gray.600'
              _hover={{ color: 'gray.400' }}
              ml='1.5'
              fontSize='sm'
            >
              <MdClose size={14} />
            </Toast.CloseTrigger>
          </Toast.Root>
        )}
      </ChakraToaster>
    </Portal>
  )
}
