import type { ComponentProps } from 'react'

import { Button } from '@/components/ui/button'

export function FooterActionButton(props: ComponentProps<typeof Button>) {
  return (
    <Button
      size='sm'
      variant='ghost'
      height='32px'
      minWidth='32px'
      bg='whiteAlpha.50'
      px={1.5}
      borderRadius='md'
      border='1px solid'
      borderColor='app.border'
      _hover={{ bg: 'whiteAlpha.100' }}
      {...props}
    />
  )
}
