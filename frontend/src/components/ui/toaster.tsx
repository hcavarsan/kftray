import { useEffect, useRef } from 'react'
import { X } from 'lucide-react'

import {
  Toaster as ChakraToaster,
  createToaster,
  Portal,
  Spinner,
  Stack,
  Toast,
} from '@chakra-ui/react'

export const toaster = createToaster({
  placement: 'top-end',
  duration: 1000,
  overlap: true,
  offsets: { top: '5px', right: '5px', bottom: '5px', left: '5px' },
})
export function Toaster() {
  const toastRef = useRef<HTMLDivElement>(null)
  const dismissTimeoutRef = useRef<ReturnType<typeof setTimeout>>(undefined)

  useEffect(() => {
    const handleMouseDown = (event: MouseEvent) => {
      if (!toastRef.current) {
        return
      }
      clearTimeout(dismissTimeoutRef.current)
      dismissTimeoutRef.current = setTimeout(() => {
        if (
          toastRef.current &&
          event.target instanceof Node &&
          !toastRef.current.contains(event.target)
        ) {
          toaster.dismiss()
        }
      }, 200)
    }
    document.addEventListener('mousedown', handleMouseDown)
    return () => {
      clearTimeout(dismissTimeoutRef.current)
      document.removeEventListener('mousedown', handleMouseDown)
    }
  }, [])

  return (
    <Portal>
      <ChakraToaster
        toaster={toaster}
        insetInline={{ mdDown: '2' }}
        insetBlock={{ mdDown: '2' }}
        css={{ pointerEvents: 'none' }}
      >
        {toast => (
          <Toast.Root
            ref={toastRef}
            width={{ base: '240px', md: '260px' }}
            maxWidth='calc(100vw - 16px)'
            py='2'
            px='3'
            bg='gray.900'
            borderRadius='lg'
            boxShadow='dark-lg'
            border='1px solid'
            borderColor='gray.800'
            css={{ pointerEvents: 'auto' }}
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
              <X size={14} />
            </Toast.CloseTrigger>
          </Toast.Root>
        )}
      </ChakraToaster>
    </Portal>
  )
}
