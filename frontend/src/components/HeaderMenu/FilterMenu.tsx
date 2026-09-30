import { useMemo } from 'react'
import { Filter } from 'lucide-react'

import { Box, Flex, Menu, Portal, Text } from '@chakra-ui/react'

import ActiveFilterChips from '@/components/HeaderMenu/ActiveFilterChips'
import {
  contentProps,
  itemProps,
  sectionLabelProps,
  separatorProps,
  ToolbarIconButton,
  type ViewMenuProps,
  WithTooltip,
} from '@/components/HeaderMenu/ToolbarParts'
import {
  fieldLabel,
  isTagField,
  toggleFilterAny,
  toggleFilterValue,
} from '@/components/PortForwardTable/viewUtils'
import type { ViewCondition } from '@/types'

const Count = ({ value }: { value: number }) => (
  <Text as='span' ms='auto' ps={3} fontSize='10px' color='whiteAlpha.400'>
    {value}
  </Text>
)

export const FilterMenu = ({ view, facets, setView }: ViewMenuProps) => {
  const filterCount = view.filters.length
  const setFilters = (filters: ViewCondition[]) => setView({ ...view, filters })
  const hasValue = (field: string, value: string) =>
    view.filters.some(c => c.field === field && c.values.includes(value))
  const hasAny = (field: string) =>
    view.filters.some(c => c.field === field && c.values.length === 0)
  const sections = useMemo(
    () =>
      facets.filter(
        facet =>
          isTagField(facet.field) ||
          facet.values.length > 1 ||
          view.filters.some(c => c.field === facet.field),
      ),
    [facets, view.filters],
  )

  return (
    <Menu.Root closeOnSelect={false} positioning={{ placement: 'bottom-end' }}>
      <WithTooltip
        content={filterCount > 0 ? 'Edit filters' : 'Filter configs'}
      >
        <Menu.Trigger asChild>
          <ToolbarIconButton
            aria-label='Filter'
            active={filterCount > 0}
            badge={filterCount}
          >
            <Filter size={13} />
          </ToolbarIconButton>
        </Menu.Trigger>
      </WithTooltip>
      <Portal>
        <Menu.Positioner>
          <Menu.Content {...contentProps}>
            <Box
              position='sticky'
              top={-1}
              zIndex={1}
              mt={-1}
              pt={1}
              mb={filterCount > 0 ? 1 : 0}
              bg='app.raised'
              borderBottom={filterCount > 0 ? '1px solid' : 'none'}
              borderBottomColor='app.subtle'
            >
              <Flex
                align='center'
                justify='space-between'
                gap={3}
                ps={3.5}
                pe={2}
              >
                <Text {...sectionLabelProps} ps={0} pe={0} whiteSpace='nowrap'>
                  {filterCount > 0 ? 'Active filters' : 'Filter by'}
                </Text>
                {filterCount > 0 && (
                  <Menu.Item
                    value='clear'
                    onClick={() => setFilters([])}
                    fontSize='10px'
                    color='blue.300'
                    flex='none'
                    w='auto'
                    whiteSpace='nowrap'
                    minH='auto'
                    px={1.5}
                    py={0.5}
                    borderRadius='sm'
                    bg='transparent'
                    _highlighted={{ bg: 'whiteAlpha.100' }}
                  >
                    Clear all
                  </Menu.Item>
                )}
              </Flex>
              <ActiveFilterChips view={view} setView={setView} />
            </Box>
            {sections.length === 0 && (
              <Text fontSize='11px' color='whiteAlpha.500' px={3.5} py={1.5}>
                Nothing to filter yet
              </Text>
            )}
            {sections.map((facet, index) => (
              <Menu.ItemGroup key={facet.field}>
                {index > 0 && <Menu.Separator {...separatorProps} />}
                <Menu.ItemGroupLabel {...sectionLabelProps}>
                  {fieldLabel(facet.field)}
                </Menu.ItemGroupLabel>
                {isTagField(facet.field) && (
                  <Menu.CheckboxItem
                    value={`${facet.field}:any`}
                    checked={hasAny(facet.field)}
                    onCheckedChange={() =>
                      setFilters(toggleFilterAny(view.filters, facet.field))
                    }
                    {...itemProps}
                  >
                    <Menu.ItemIndicator />
                    Has tag
                    <Count value={facet.count} />
                  </Menu.CheckboxItem>
                )}
                {facet.values.map(({ value, count }) => (
                  <Menu.CheckboxItem
                    key={value}
                    value={`${facet.field}=${value}`}
                    checked={hasValue(facet.field, value)}
                    onCheckedChange={() =>
                      setFilters(
                        toggleFilterValue(view.filters, facet.field, value),
                      )
                    }
                    {...itemProps}
                  >
                    <Menu.ItemIndicator />
                    <Text as='span' truncate maxW='150px'>
                      {value}
                    </Text>
                    <Count value={count} />
                  </Menu.CheckboxItem>
                ))}
              </Menu.ItemGroup>
            ))}
          </Menu.Content>
        </Menu.Positioner>
      </Portal>
    </Menu.Root>
  )
}
