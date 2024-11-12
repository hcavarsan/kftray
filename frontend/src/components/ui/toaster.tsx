'use client'

import React from 'react'

import {
  createToaster,
  Portal,
  Stack,
  Toast,
  Toaster as ChakraToaster,
} from '@chakra-ui/react'

// Create the toaster instance with default configuration
export const toaster = createToaster({
  placement: 'top-end', // Changed from 'top-right' to 'top-end'
  pauseOnPageIdle: true,
})

// Custom Toast Component
const CustomToast = ({
  title,
  description,
  type,
}: {
  title?: string
  description?: string
  type?: 'info' | 'warning' | 'success' | 'error'
}) => {
  const statusColorMap: Record<string, string> = {
    info: 'blue.700',
    warning: 'orange.700',
    success: 'green.700',
    error: 'red.700',
  }

  const backgroundColor = type ? statusColorMap[type] : 'gray.500'

  return (
    <Toast.Root
      width={{ md: 'sm' }}
      bg={backgroundColor}
      color='white'
      borderRadius='lg'
      boxShadow='md'
    >
      <Stack gap='1' flex='1' maxWidth='100%' p='3'>
        {title && (
          <Toast.Title fontSize='12px' fontWeight='bold'>
            {title}
          </Toast.Title>
        )}
        {description && (
          <Toast.Description fontSize='10px'>{description}</Toast.Description>
        )}
      </Stack>
      <Toast.CloseTrigger color='white' />
    </Toast.Root>
  )
}

// Toaster Component
export const Toaster = () => {
  return (
    <Portal>
      <ChakraToaster toaster={toaster} insetInline={{ mdDown: '4' }}>
        {toast => (
          <CustomToast
            title={String(toast.title || '')}
            description={String(toast.description || '')}
            type={toast.type as 'info' | 'warning' | 'success' | 'error'}
          />
        )}
      </ChakraToaster>
    </Portal>
  )
}

// Custom hook for using the toaster
export const useCustomToast = () => {
  const showToast = ({
    title,
    description = '',
    status = 'error',
    duration = 3000,
  }: {
    title: string
    description?: string
    status?: 'info' | 'warning' | 'success' | 'error'
    duration?: number
  }) => {
    toaster.create({
      title,
      description,
      type: status,
      duration,
      meta: { closable: true },
    })
  }

  return showToast
}
