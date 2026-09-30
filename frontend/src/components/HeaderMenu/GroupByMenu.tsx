import { useMemo } from 'react'
import { Layers } from 'lucide-react'

import { Menu, Portal } from '@chakra-ui/react'

import {
  contentProps,
  itemProps,
  sectionLabelProps,
  separatorProps,
  ToolbarIconButton,
  type ViewMenuProps,
  WithTooltip,
} from '@/components/HeaderMenu/ToolbarParts'
import { fieldLabel, isTagField } from '@/components/PortForwardTable/viewUtils'

const NO_GROUP = '__none__'

export const GroupByMenu = ({ view, facets, setView }: ViewMenuProps) => {
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
