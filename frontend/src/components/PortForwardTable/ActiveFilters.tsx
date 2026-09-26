import type React from 'react'
import { X } from 'lucide-react'

import { chakra, Flex, Text } from '@chakra-ui/react'

import type { ConfigView, ViewCondition } from '@/types'

import {
  fieldLabel,
  isTagField,
  toggleFilterAny,
  toggleFilterValue,
} from './viewUtils'

interface ActiveFiltersProps {
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

const ActiveFilters: React.FC<ActiveFiltersProps> = ({ view, setView }) => {
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
    <Flex wrap='wrap' align='center' gap={1} px={1} pt={1.5}>
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
            bg='rgba(59, 130, 246, 0.1)'
            border='1px solid rgba(59, 130, 246, 0.25)'
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
              _hover={{ bg: 'rgba(59, 130, 246, 0.25)', color: 'white' }}
              _focusVisible={{ outline: '1px solid rgb(59, 130, 246)' }}
              onClick={() => remove(chip)}
            >
              <X size={9} />
            </chakra.button>
          </Flex>
        )
      })}
      <chakra.button
        type='button'
        fontSize='10px'
        color='whiteAlpha.500'
        cursor='pointer'
        px={1}
        h='18px'
        borderRadius='sm'
        _hover={{ color: 'whiteAlpha.900' }}
        _focusVisible={{ outline: '1px solid rgb(59, 130, 246)' }}
        onClick={() => setFilters([])}
      >
        Clear
      </chakra.button>
    </Flex>
  )
}

export default ActiveFilters
