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
      formatted = `… ${segments.slice(-3).join(' › ')}`
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
          bg={hasSelection ? 'cyan.400/10' : 'transparent'}
          color={hasSelection ? 'cyan.300' : 'whiteAlpha.600'}
          border='1px solid'
          borderColor={hasSelection ? 'cyan.400/20' : 'app.border'}
          _hover={{
            bg: hasSelection ? 'cyan.400/15' : 'whiteAlpha.50',
            borderColor: hasSelection ? 'cyan.400/30' : 'app.borderStrong',
          }}
        >
          <Layers size={12} />
          {hasSelection && (
            <Box
              position='absolute'
              top='-3px'
              right='-3px'
              bg='cyan.300'
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
            bg='app.panel'
            border='1px solid'
            borderColor='app.border'
            minW='220px'
            maxH='280px'
            py={0.5}
          >
            <Box
              px={2}
              py={1}
              borderBottom='1px solid'
              borderBottomColor='app.hover'
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
                    color='blue.500'
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
                bg='app.raised'
                border='1px solid'
                borderColor='app.border'
                color='whiteAlpha.900'
                _placeholder={{ color: 'whiteAlpha.400' }}
                _hover={{ borderColor: 'app.borderStrong' }}
                _focus={{ borderColor: 'blue.500', boxShadow: 'none' }}
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
                            borderColor='app.border'
                            _checked={{
                              bg: 'cyan.300',
                              borderColor: 'cyan.300',
                            }}
                          >
                            <Checkbox.Indicator />
                          </Checkbox.Control>
                        </Checkbox.Root>
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
