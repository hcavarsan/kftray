import { memo, useCallback } from 'react'
import { Filter } from 'lucide-react'

import {
  Box,
  Checkbox,
  Flex,
  IconButton,
  Menu,
  Portal,
  Text,
} from '@chakra-ui/react'

import { ALL_LEVELS, LEVEL_COLORS } from './constants'
import type { LevelFilterDropdownProps, LogLevel } from './types'

function LevelFilterDropdownComponent({
  selectedLevels,
  onLevelChange,
}: LevelFilterDropdownProps) {
  const handleToggle = useCallback(
    (level: LogLevel) => {
      if (selectedLevels.includes(level)) {
        onLevelChange(selectedLevels.filter(l => l !== level))
      } else {
        onLevelChange([...selectedLevels, level])
      }
    },
    [selectedLevels, onLevelChange],
  )

  const handleSelectAll = useCallback(() => {
    onLevelChange([...ALL_LEVELS])
  }, [onLevelChange])

  const handleClearAll = useCallback(() => {
    onLevelChange([])
  }, [onLevelChange])

  const hasSelection = selectedLevels.length > 0

  return (
    <Menu.Root>
      <Menu.Trigger asChild>
        <IconButton
          aria-label='Filter by level'
          size='xs'
          variant='ghost'
          h='24px'
          w='24px'
          minW='24px'
          position='relative'
          bg={hasSelection ? 'blue.500/10' : 'transparent'}
          color={hasSelection ? 'blue.500' : 'whiteAlpha.600'}
          border='1px solid'
          borderColor={hasSelection ? 'app.accentMuted' : 'app.border'}
          _hover={{
            bg: hasSelection ? 'app.accentSubtle' : 'whiteAlpha.50',
            borderColor: hasSelection ? 'blue.500/40' : 'app.borderStrong',
          }}
        >
          <Filter size={12} />
          {hasSelection && (
            <Box
              position='absolute'
              top='-3px'
              right='-3px'
              bg='blue.500'
              color='white'
              fontSize='8px'
              fontWeight='bold'
              borderRadius='full'
              w='12px'
              h='12px'
              display='flex'
              alignItems='center'
              justifyContent='center'
            >
              {selectedLevels.length}
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
            minW='120px'
            py={0.5}
          >
            <Box
              px={2}
              py={1}
              borderBottom='1px solid'
              borderBottomColor='app.hover'
            >
              <Flex justify='space-between' align='center'>
                <Text
                  fontSize='10px'
                  color='whiteAlpha.500'
                  fontWeight='medium'
                >
                  Levels
                </Text>
                <Flex gap={1}>
                  <Text
                    fontSize='9px'
                    color='blue.500'
                    cursor='pointer'
                    onClick={handleSelectAll}
                    _hover={{ textDecoration: 'underline' }}
                  >
                    All
                  </Text>
                  <Text fontSize='9px' color='whiteAlpha.300'>
                    |
                  </Text>
                  <Text
                    fontSize='9px'
                    color='blue.500'
                    cursor='pointer'
                    onClick={handleClearAll}
                    _hover={{ textDecoration: 'underline' }}
                  >
                    None
                  </Text>
                </Flex>
              </Flex>
            </Box>
            {ALL_LEVELS.map(level => {
              const colors = LEVEL_COLORS[level]
              const isSelected = selectedLevels.includes(level)

              return (
                <Menu.Item
                  key={level}
                  value={level}
                  onClick={() => handleToggle(level)}
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
                          bg: 'blue.500',
                          borderColor: 'blue.500',
                        }}
                      >
                        <Checkbox.Indicator />
                      </Checkbox.Control>
                    </Checkbox.Root>
                    <Text
                      fontSize='10px'
                      fontWeight='medium'
                      fontFamily='mono'
                      color={colors.text}
                    >
                      {level}
                    </Text>
                  </Flex>
                </Menu.Item>
              )
            })}
          </Menu.Content>
        </Menu.Positioner>
      </Portal>
    </Menu.Root>
  )
}

export const LevelFilterDropdown = memo(LevelFilterDropdownComponent)
