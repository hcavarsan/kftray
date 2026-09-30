import type { ComponentProps, ReactNode } from 'react'
import { useMemo } from 'react'
import { Filter, Layers } from 'lucide-react'

import { Box, Flex, IconButton, Menu, Portal, Text } from '@chakra-ui/react'

import ActiveFilterChips from '@/components/HeaderMenu/ActiveFilterChips'
import {
  fieldLabel,
  isTagField,
  toggleFilterAny,
  toggleFilterValue,
} from '@/components/PortForwardTable/viewUtils'
import { Tooltip } from '@/components/ui/tooltip'
import type { ConfigView, Facet, ViewCondition } from '@/types'

const NO_GROUP = '__none__'

interface ToolbarIconButtonProps extends ComponentProps<typeof IconButton> {
  active?: boolean
  badge?: number
}

export const ToolbarIconButton = ({
  active = false,
  badge = 0,
  children,
  ...rest
}: ToolbarIconButtonProps) => (
  <IconButton
    size='xs'
    variant='ghost'
    h='26px'
    w='26px'
    minW='26px'
    position='relative'
    overflow='visible'
    borderRadius='md'
    border='1px solid'
    borderColor={active ? 'blue.500/35' : 'app.border'}
    bg={active ? 'blue.500/12' : 'whiteAlpha.50'}
    color={active ? 'blue.400' : 'whiteAlpha.700'}
    _hover={{
      bg: active ? 'blue.500/18' : 'whiteAlpha.100',
      color: active ? 'app.accentText' : 'whiteAlpha.900',
    }}
    _focusVisible={{
      outline: '1px solid',
      outlineColor: 'blue.500',
      outlineOffset: '1px',
    }}
    {...rest}
  >
    {children}
    {badge > 0 && (
      <Box
        position='absolute'
        top='-6px'
        right='-6px'
        minW='12px'
        h='12px'
        px='3px'
        bg='blue.500'
        color='white'
        fontSize='8px'
        fontWeight='semibold'
        lineHeight='12px'
        textAlign='center'
        borderRadius='full'
        boxShadow='0 0 0 1.5px var(--chakra-colors-app-panel)'
        pointerEvents='none'
      >
        {badge}
      </Box>
    )}
  </IconButton>
)

export const WithTooltip = ({
  content,
  children,
}: {
  content: string
  children: ReactNode
}) => (
  <Tooltip content={content}>
    <Box display='inline-flex'>{children}</Box>
  </Tooltip>
)

const contentProps = {
  bg: 'app.raised',
  border: '1px solid',
  borderColor: 'app.border',
  borderRadius: 'md',
  boxShadow: 'popover',
  minW: '200px',
  maxW: '260px',
  maxH: '300px',
  overflowY: 'auto' as const,
  py: 1,
}

const itemProps = {
  fontSize: '11px',
  minH: '24px',
  py: 0,
  ps: 7,
  pe: 2.5,
  borderRadius: 'sm',
  mx: 1,
  color: 'whiteAlpha.900',
  bg: 'transparent',
  _highlighted: { bg: 'whiteAlpha.100' },
}

const sectionLabelProps = {
  fontSize: '10px',
  fontWeight: 'medium',
  color: 'whiteAlpha.500',
  ps: 3.5,
  pe: 2.5,
  pt: 1.5,
  pb: 0.5,
}

const separatorProps = {
  my: 1,
  borderColor: 'app.subtle',
}

const Count = ({ value }: { value: number }) => (
  <Text as='span' ms='auto' ps={3} fontSize='10px' color='whiteAlpha.400'>
    {value}
  </Text>
)

interface ViewControlsProps {
  view: ConfigView | null
  facets: Facet[]
  setView: (view: ConfigView) => void
}

const GroupByMenu = ({
  view,
  facets,
  setView,
}: Required<Omit<ViewControlsProps, 'view'>> & { view: ConfigView }) => {
  const fields = useMemo(
    () =>
      facets
        .filter(f => !isTagField(f.field) && f.values.length > 0)
        .map(f => f.field),
    [facets],
  )
  const tags = useMemo(
    () => facets.filter(f => isTagField(f.field)).map(f => f.field),
    [facets],
  )
  const current = view.group_by ? fieldLabel(view.group_by) : 'nothing'

  return (
    <Menu.Root positioning={{ placement: 'bottom-end' }}>
      <WithTooltip content={`Grouped by ${current.toLowerCase()}`}>
        <Menu.Trigger asChild>
          <ToolbarIconButton
            aria-label='Group by'
            active={view.group_by !== 'context'}
          >
            <Layers size={13} />
          </ToolbarIconButton>
        </Menu.Trigger>
      </WithTooltip>
      <Portal>
        <Menu.Positioner>
          <Menu.Content {...contentProps}>
            <Menu.RadioItemGroup
              value={view.group_by ?? NO_GROUP}
              onValueChange={({ value }) =>
                setView({
                  ...view,
                  group_by: value === NO_GROUP ? null : value,
                })
              }
            >
              <Menu.ItemGroupLabel {...sectionLabelProps}>
                Group by
              </Menu.ItemGroupLabel>
              {fields.map(field => (
                <Menu.RadioItem key={field} value={field} {...itemProps}>
                  <Menu.ItemIndicator />
                  {fieldLabel(field)}
                </Menu.RadioItem>
              ))}
              {tags.length > 0 && (
                <>
                  <Menu.Separator {...separatorProps} />
                  <Menu.ItemGroupLabel {...sectionLabelProps}>
                    Tags
                  </Menu.ItemGroupLabel>
                  {tags.map(field => (
                    <Menu.RadioItem key={field} value={field} {...itemProps}>
                      <Menu.ItemIndicator />
                      {field.slice(4)}
                    </Menu.RadioItem>
                  ))}
                </>
              )}
              <Menu.Separator {...separatorProps} />
              <Menu.RadioItem value={NO_GROUP} {...itemProps}>
                <Menu.ItemIndicator />
                No grouping
              </Menu.RadioItem>
            </Menu.RadioItemGroup>
          </Menu.Content>
        </Menu.Positioner>
      </Portal>
    </Menu.Root>
  )
}

const FilterMenu = ({
  view,
  facets,
  setView,
}: Required<Omit<ViewControlsProps, 'view'>> & { view: ConfigView }) => {
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

const ViewControls = ({ view, facets, setView }: ViewControlsProps) => {
  if (!view) {
    return null
  }

  return (
    <>
      <GroupByMenu view={view} facets={facets} setView={setView} />
      <FilterMenu view={view} facets={facets} setView={setView} />
    </>
  )
}

export default ViewControls
