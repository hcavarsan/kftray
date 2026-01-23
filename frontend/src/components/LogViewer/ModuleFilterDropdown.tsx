import { memo, useCallback, useMemo, useState } from 'react'
import { Layers } from 'lucide-react'

import {
  Box,
  Checkbox,
  Flex,
  IconButton,
  Input,
  Menu,
  Portal,
  Text,
} from '@chakra-ui/react'

import { COLORS } from './constants'
import type { ModuleFilterDropdownProps } from './types'

function ModuleFilterDropdownComponent({
  availableModules,
  selectedModules,
  onModuleChange,
}: ModuleFilterDropdownProps) {
  const [searchText, setSearchText] = useState('')

  const filteredModules = useMemo(() => {
    if (!searchText.trim()) {
      return availableModules
    }
    const search = searchText.toLowerCase()

    return availableModules.filter(m => m.toLowerCase().includes(search))
  }, [availableModules, searchText])

  const handleToggle = useCallback(
    (module: string) => {
      if (selectedModules.includes(module)) {
        onModuleChange(selectedModules.filter(m => m !== module))
      } else {
        onModuleChange([...selectedModules, module])
      }
    },
    [selectedModules, onModuleChange],
  )

  const handleClearAll = useCallback(() => {
    onModuleChange([])
  }, [onModuleChange])

  const formatModuleName = (module: string): string => {
    let formatted = module
      .replace(/^kftray_portforward::/, '')
      .replace(/^kftray_tauri::/, '')
      .replace(/^kftray_/, '')

    const segments = formatted.split('::')

    if (segments.length > 3) {
      formatted = '… ' + segments.slice(-3).join(' › ')
    } else {
      formatted = segments.join(' › ')
    }

    return formatted
  }

  const hasSelection = selectedModules.length > 0

  return (
    <Menu.Root>
      <Menu.Trigger asChild>
        <IconButton
          aria-label='Filter by module'
          size='xs'
          variant='ghost'
          h='24px'
          w='24px'
          minW='24px'
          position='relative'
          bg={hasSelection ? 'rgba(34, 211, 238, 0.1)' : 'transparent'}
          color={hasSelection ? COLORS.accentCyan : 'whiteAlpha.600'}
          border='1px solid'
          borderColor={
            hasSelection ? 'rgba(34, 211, 238, 0.2)' : COLORS.borderDefault
          }
          _hover={{
            bg: hasSelection ? 'rgba(34, 211, 238, 0.15)' : 'whiteAlpha.50',
            borderColor: hasSelection
              ? 'rgba(34, 211, 238, 0.3)'
              : COLORS.borderHover,
          }}
        >
          <Layers size={12} />
          {hasSelection && (
            <Box
              position='absolute'
              top='-3px'
              right='-3px'
              bg={COLORS.accentCyan}
              color='black'
              fontSize='8px'
              fontWeight='bold'
              borderRadius='full'
              w='12px'
              h='12px'
              display='flex'
              alignItems='center'
              justifyContent='center'
            >
              {selectedModules.length}
            </Box>
          )}
        </IconButton>
      </Menu.Trigger>
      <Portal>
        <Menu.Positioner>
          <Menu.Content
            bg={COLORS.bgSecondary}
            border='1px solid'
            borderColor={COLORS.borderDefault}
            minW='220px'
            maxH='280px'
            py={0.5}
          >
            <Box
              px={2}
              py={1}
              borderBottom='1px solid'
              borderBottomColor={COLORS.borderSubtle}
            >
              <Flex justify='space-between' align='center' mb={1}>
                <Text
                  fontSize='10px'
                  color='whiteAlpha.500'
                  fontWeight='medium'
                >
                  Modules
                </Text>
                {hasSelection && (
                  <Text
                    fontSize='9px'
                    color={COLORS.accentBlue}
                    cursor='pointer'
                    onClick={handleClearAll}
                    _hover={{ textDecoration: 'underline' }}
                  >
                    Clear ({selectedModules.length})
                  </Text>
                )}
              </Flex>
              <Input
                placeholder='Search...'
                size='xs'
                value={searchText}
                onChange={e => setSearchText(e.target.value)}
                height='22px'
                fontSize='10px'
                bg={COLORS.bgInput}
                border='1px solid'
                borderColor={COLORS.borderDefault}
                color='whiteAlpha.900'
                _placeholder={{ color: 'whiteAlpha.400' }}
                _hover={{ borderColor: COLORS.borderHover }}
                _focus={{ borderColor: COLORS.accentBlue, boxShadow: 'none' }}
              />
            </Box>
            <Box maxH='200px' overflowY='auto'>
              {filteredModules.length === 0 ? (
                <Box px={2} py={1}>
                  <Text fontSize='10px' color='whiteAlpha.400'>
                    {availableModules.length === 0 ? 'No modules' : 'No match'}
                  </Text>
                </Box>
              ) : (
                filteredModules.map(module => {
                  const isSelected = selectedModules.includes(module)

                  return (
                    <Menu.Item
                      key={module}
                      value={module}
                      onClick={() => handleToggle(module)}
                      bg='transparent'
                      _hover={{ bg: 'whiteAlpha.50' }}
                      py={1}
                      px={2}
                    >
                      <Flex align='center' gap={1.5} w='100%'>
                        <Checkbox.Root checked={isSelected} size='sm'>
                          <Checkbox.HiddenInput />
                          <Checkbox.Control
                            borderColor={COLORS.borderDefault}
                            _checked={{
                              bg: COLORS.accentCyan,
                              borderColor: COLORS.accentCyan,
                            }}
                          >
                            <Checkbox.Indicator />
                          </Checkbox.Control>
                        </Checkbox.Root>
                        <Text
                          fontSize='10px'
                          fontFamily='mono'
                          color={COLORS.accentCyan}
                          overflow='hidden'
                          textOverflow='ellipsis'
                          whiteSpace='nowrap'
                          title={module}
                        >
                          {formatModuleName(module)}
                        </Text>
                      </Flex>
                    </Menu.Item>
                  )
                })
              )}
            </Box>
          </Menu.Content>
        </Menu.Positioner>
      </Portal>
    </Menu.Root>
  )
}

export const ModuleFilterDropdown = memo(ModuleFilterDropdownComponent)
