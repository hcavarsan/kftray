import { X } from 'lucide-react'

import { chakra, Flex, Text } from '@chakra-ui/react'

import {
  fieldLabel,
  isTagField,
  toggleFilterAny,
  toggleFilterValue,
} from '@/components/PortForwardTable/viewUtils'
import type { ConfigView, ViewCondition } from '@/types'

interface ActiveFilterChipsProps {
  view: ConfigView
  setView: (view: ConfigView) => void
}

interface FilterChip {
  field: string
  value: string | null
}

const chips = (filters: ViewCondition[]): FilterChip[] =>
  filters.flatMap<FilterChip>(({ field, values }) =>
    values.length
      ? values.map(value => ({ field, value }))
      : [{ field, value: null }],
  )

const chipLabel = ({ field, value }: FilterChip) => {
  const name = isTagField(field) ? field.slice(4) : fieldLabel(field)

  return value === null
    ? { name: 'has', value: name }
    : { name: `${name}:`, value }
}

function ActiveFilterChips({ view, setView }: ActiveFilterChipsProps) {
  if (view.filters.length === 0) {
    return null
  }

  const setFilters = (filters: ViewCondition[]) => setView({ ...view, filters })
  const remove = ({ field, value }: FilterChip) =>
    setFilters(
      value === null
        ? toggleFilterAny(view.filters, field)
        : toggleFilterValue(view.filters, field, value),
    )

  return (
    <Flex wrap='wrap' align='center' gap={1} ps={3.5} pe={2.5} pt={0.5} pb={2}>
      {chips(view.filters).map(chip => {
        const label = chipLabel(chip)

        return (
          <Flex
            key={`${chip.field}=${chip.value ?? ''}`}
            align='center'
            gap={1}
            h='18px'
            ps={1.5}
            pe={0.5}
            borderRadius='sm'
            bg='blue.500/10'
            border='1px solid'
            borderColor='blue.500/25'
            fontSize='10px'
            lineHeight='1'
          >
            <Text as='span' color='whiteAlpha.600'>
              {label.name}
            </Text>
            <Text as='span' color='rgb(147, 197, 253)' truncate maxW='120px'>
              {label.value}
            </Text>
            <chakra.button
              type='button'
              aria-label={`Remove filter ${label.name} ${label.value}`}
              display='inline-flex'
              alignItems='center'
              justifyContent='center'
              w='14px'
              h='14px'
              borderRadius='sm'
              color='whiteAlpha.500'
              cursor='pointer'
              _hover={{ bg: 'blue.500/25', color: 'white' }}
              _focusVisible={{ outline: '1px solid rgb(59, 130, 246)' }}
              onClick={() => remove(chip)}
            >
              <X size={9} />
            </chakra.button>
          </Flex>
        )
      })}
    </Flex>
  )
}

export default ActiveFilterChips
