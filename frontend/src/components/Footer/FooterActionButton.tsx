import type { ComponentProps } from 'react'

import { Button } from '@/components/ui/button'

export function FooterActionButton(props: ComponentProps<typeof Button>) {
  return (
    <Button
      size='sm'
      variant='ghost'
      height='32px'
      minWidth='32px'
      bg='bg.faint'
      px={1.5}
      borderRadius='md'
      border='1px solid'
      borderColor='border'
      _hover={{ bg: 'bg.hover' }}
      {...props}
    />
  )
}
