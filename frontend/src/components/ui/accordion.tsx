import { forwardRef } from 'react'
import { LuChevronDown } from 'react-icons/lu'

import { Accordion, HStack } from '@chakra-ui/react'

interface AccordionItemTriggerProps extends Accordion.ItemTriggerProps {
  indicatorPlacement?: 'start' | 'end'
}

export const AccordionItemTrigger = forwardRef<
  HTMLButtonElement,
  AccordionItemTriggerProps
>((props, ref) => {
  const { children, indicatorPlacement = 'end', ...rest } = props

  return (
    <Accordion.ItemTrigger {...rest} ref={ref}>
      {indicatorPlacement === 'start' && (
        <Accordion.ItemIndicator rotate={{ base: '-90deg', _open: '0deg' }}>
          <LuChevronDown />
        </Accordion.ItemIndicator>
      )}
      <HStack gap='4' flex='1' textAlign='start' width='full'>
        {children}
      </HStack>
      {indicatorPlacement === 'end' && (
        <Accordion.ItemIndicator>
          <LuChevronDown />
        </Accordion.ItemIndicator>
      )}
    </Accordion.ItemTrigger>
  )
})

interface AccordionItemContentProps extends Accordion.ItemContentProps {}

export const AccordionItemContent = forwardRef<
  HTMLDivElement,
  AccordionItemContentProps
>((props, ref) => {
  return (
    <Accordion.ItemContent>
      <Accordion.ItemBody {...props} ref={ref} />
    </Accordion.ItemContent>
  )
})

export interface ValueChangeDetails {
  value: string[]
}

export const AccordionRoot = Accordion.Root
export const AccordionItem = Accordion.Item
