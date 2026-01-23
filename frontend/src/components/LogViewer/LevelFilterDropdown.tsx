import { memo, useCallback } from 'react'
import { Filter } from 'lucide-react'

import { Box, Checkbox, Flex, IconButton, Menu, Portal, Text } from '@chakra-ui/react'

import { ALL_LEVELS, COLORS, LEVEL_COLORS } from './constants'
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
    [selectedLevels, onLevelChange]
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
          aria-label="Filter by level"
          size="xs"
          variant="ghost"
          h="24px"
          w="24px"
          minW="24px"
          position="relative"
          bg={hasSelection ? 'rgba(59, 130, 246, 0.1)' : 'transparent'}
          color={hasSelection ? COLORS.accentBlue : 'whiteAlpha.600'}
          border="1px solid"
          borderColor={hasSelection ? 'rgba(59, 130, 246, 0.3)' : COLORS.borderDefault}
          _hover={{
            bg: hasSelection ? 'rgba(59, 130, 246, 0.15)' : 'whiteAlpha.50',
            borderColor: hasSelection ? 'rgba(59, 130, 246, 0.4)' : COLORS.borderHover,
          }}
        >
          <Filter size={12} />
          {hasSelection && (
            <Box
              position="absolute"
              top="-3px"
              right="-3px"
              bg={COLORS.accentBlue}
              color="white"
              fontSize="8px"
              fontWeight="bold"
              borderRadius="full"
              w="12px"
              h="12px"
              display="flex"
              alignItems="center"
              justifyContent="center"
            >
              {selectedLevels.length}
            </Box>
          )}
        </IconButton>
      </Menu.Trigger>
      <Portal>
        <Menu.Positioner>
          <Menu.Content
            bg={COLORS.bgSecondary}
            border="1px solid"
            borderColor={COLORS.borderDefault}
            minW="120px"
            py={0.5}
          >
            <Box px={2} py={1} borderBottom="1px solid" borderBottomColor={COLORS.borderSubtle}>
              <Flex justify="space-between" align="center">
                <Text fontSize="10px" color="whiteAlpha.500" fontWeight="medium">
                  Levels
                </Text>
                <Flex gap={1}>
                  <Text
                    fontSize="9px"
                    color={COLORS.accentBlue}
                    cursor="pointer"
                    onClick={handleSelectAll}
                    _hover={{ textDecoration: 'underline' }}
                  >
                    All
                  </Text>
                  <Text fontSize="9px" color="whiteAlpha.300">|</Text>
                  <Text
                    fontSize="9px"
                    color={COLORS.accentBlue}
                    cursor="pointer"
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
                  bg="transparent"
                  _hover={{ bg: 'whiteAlpha.50' }}
                  py={1}
                  px={2}
                >
                  <Flex align="center" gap={1.5} w="100%">
                    <Checkbox.Root checked={isSelected} size="sm">
                      <Checkbox.HiddenInput />
                      <Checkbox.Control
                        borderColor={COLORS.borderDefault}
                        _checked={{ bg: COLORS.accentBlue, borderColor: COLORS.accentBlue }}
                      >
                        <Checkbox.Indicator />
                      </Checkbox.Control>
                    </Checkbox.Root>
                    <Text
                      fontSize="10px"
                      fontWeight="medium"
                      fontFamily="mono"
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
