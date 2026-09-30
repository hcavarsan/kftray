import type { RefAttributes } from 'react'
import { ChevronDown } from 'lucide-react'

import { Accordion, HStack } from '@chakra-ui/react'

interface AccordionItemTriggerProps
  extends Accordion.ItemTriggerProps,
    RefAttributes<HTMLButtonElement> {
  indicatorPlacement?: 'start' | 'end'
}

export function AccordionItemTrigger({
  children,
  indicatorPlacement = 'end',
  ref,
  ...props
}: AccordionItemTriggerProps) {
  return (
    <Accordion.ItemTrigger {...props} ref={ref}>
      {indicatorPlacement === 'start' && (
        <Accordion.ItemIndicator rotate={{ base: '-90deg', _open: '0deg' }}>
          <ChevronDown />
        </Accordion.ItemIndicator>
      )}
      <HStack gap='4' flex='1' textAlign='start' width='full'>
        {children}
      </HStack>
      {indicatorPlacement === 'end' && (
        <Accordion.ItemIndicator>
          <ChevronDown />
        </Accordion.ItemIndicator>
      )}
    </Accordion.ItemTrigger>
  )
}

interface AccordionItemContentProps
  extends Accordion.ItemContentProps,
    RefAttributes<HTMLDivElement> {}

export function AccordionItemContent({
  ref,
  ...props
}: AccordionItemContentProps) {
  return (
    <Accordion.ItemContent>
      <Accordion.ItemBody {...props} ref={ref} />
    </Accordion.ItemContent>
  )
}

export interface ValueChangeDetails {
  value: string[]
}

export const AccordionRoot = Accordion.Root
export const AccordionItem = Accordion.Item
