import { memo } from 'react'
import { Layers } from 'lucide-react'

import { Text } from '@chakra-ui/react'

import { LogFilterDropdown } from './LogFilterDropdown'
import type { LogFilterAccent, ModuleFilterDropdownProps } from './types'

const MODULE_ACCENT: LogFilterAccent = {
  activeBg: 'log.module.subtle',
  activeColor: 'log.module.fg',
  activeBorder: 'log.module.muted',
  hoverBg: 'log.module.muted',
  hoverBorder: 'log.module.emphasized',
  badgeBg: 'log.module.solid',
  badgeColor: 'log.module.contrast',
  checkedBg: 'log.module.solid',
}

const formatModuleName = (module: string): string => {
  let formatted = module
    .replace(/^kftray_portforward::/, '')
    .replace(/^kftray_tauri::/, '')
    .replace(/^kftray_/, '')

  const segments = formatted.split('::')

  if (segments.length > 3) {
    formatted = `… ${segments.slice(-3).join(' › ')}`
  } else {
    formatted = segments.join(' › ')
  }

  return formatted
}

const renderModule = (module: string) => (
  <Text
    fontSize='10px'
    fontFamily='mono'
    color='log.module.fg'
    overflow='hidden'
    textOverflow='ellipsis'
    whiteSpace='nowrap'
    title={module}
  >
    {formatModuleName(module)}
  </Text>
)

function ModuleFilterDropdownComponent({
  availableModules,
  selectedModules,
  onModuleChange,
}: ModuleFilterDropdownProps) {
  return (
    <LogFilterDropdown
      ariaLabel='Filter by module'
      title='Modules'
      icon={<Layers size={12} />}
      accent={MODULE_ACCENT}
      items={availableModules}
      selected={selectedModules}
      onChange={onModuleChange}
      renderItem={renderModule}
      searchable
      emptyLabel='No modules'
      minW='220px'
      maxH='280px'
    />
  )
}

export const ModuleFilterDropdown = memo(ModuleFilterDropdownComponent)
