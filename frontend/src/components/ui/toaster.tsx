'use client'

import { MdClose } from 'react-icons/md'

import {
  createToaster,
  Portal,
  Spinner,
  Stack,
  Toast,
  Toaster as ChakraToaster,
} from '@chakra-ui/react'

export const toaster = createToaster({
  placement: 'top-end',
  duration: 600,
  overlap: false,
  offsets: {
    top: '5px',
    right: '5px',
    bottom: '5px',
    left: '5px',
  },
})

export const Toaster = () => {
  return (
    <Portal>
      <ChakraToaster
        toaster={toaster}
        insetInline={{ mdDown: '2' }}
        insetBlock={{ mdDown: '2' }}
      >
        {toast => (
          <Toast.Root
            width={{ base: '240px', md: '260px' }}
            maxWidth='90%'
            py='2'
            px='3'
            bg='gray.900'
            borderRadius='lg'
            boxShadow='dark-lg'
            border='1px solid'
            borderColor='gray.800'
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
