import { memo } from 'react'
import { Layers } from 'lucide-react'

import { Text } from '@chakra-ui/react'

import { LogFilterDropdown } from './LogFilterDropdown'
import type { LogFilterAccent, ModuleFilterDropdownProps } from './types'

const MODULE_ACCENT: LogFilterAccent = {
  activeBg: 'cyan.400/10',
  activeColor: 'cyan.300',
  activeBorder: 'cyan.400/20',
  hoverBg: 'cyan.400/15',
  hoverBorder: 'cyan.400/30',
  badgeBg: 'cyan.300',
  badgeColor: 'black',
  checkedBg: 'cyan.300',
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
    color='cyan.300'
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
