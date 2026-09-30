import { useCallback, useMemo, useState } from 'react'

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

import type { LogFilterDropdownProps } from './types'

interface LogFilterActionProps {
  label: string
  onClick: () => void
}

export function LogFilterAction({ label, onClick }: LogFilterActionProps) {
  return (
    <Text
      fontSize='9px'
      color='blue.500'
      cursor='pointer'
      onClick={onClick}
      _hover={{ textDecoration: 'underline' }}
    >
      {label}
    </Text>
  )
}

export function LogFilterDropdown<T extends string>({
  ariaLabel,
  title,
  icon,
  accent,
  items,
  selected,
  onChange,
  renderItem,
  headerActions,
  searchable = false,
  emptyLabel = 'No items',
  minW,
  maxH,
}: LogFilterDropdownProps<T>) {
  const [searchText, setSearchText] = useState('')

  const visibleItems = useMemo(() => {
    if (!searchable || !searchText.trim()) {
      return items
    }
    const search = searchText.toLowerCase()

    return items.filter(item => item.toLowerCase().includes(search))
  }, [items, searchable, searchText])

  const handleToggle = useCallback(
    (item: T) => {
      if (selected.includes(item)) {
        onChange(selected.filter(current => current !== item))
      } else {
        onChange([...selected, item])
      }
    },
    [selected, onChange],
  )

  const handleClearAll = useCallback(() => {
    onChange([])
  }, [onChange])

  const hasSelection = selected.length > 0

  return (
    <Menu.Root>
      <Menu.Trigger asChild>
        <IconButton
          aria-label={ariaLabel}
          size='xs'
          variant='ghost'
          h='24px'
          w='24px'
          minW='24px'
          position='relative'
          bg={hasSelection ? accent.activeBg : 'transparent'}
          color={hasSelection ? accent.activeColor : 'whiteAlpha.600'}
          border='1px solid'
          borderColor={hasSelection ? accent.activeBorder : 'app.border'}
          _hover={{
            bg: hasSelection ? accent.hoverBg : 'whiteAlpha.50',
            borderColor: hasSelection ? accent.hoverBorder : 'app.borderStrong',
          }}
        >
          {icon}
          {hasSelection && (
            <Box
              position='absolute'
              top='-3px'
              right='-3px'
              bg={accent.badgeBg}
              color={accent.badgeColor}
              fontSize='8px'
              fontWeight='bold'
              borderRadius='full'
              w='12px'
              h='12px'
              display='flex'
              alignItems='center'
              justifyContent='center'
            >
              {selected.length}
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
            minW={minW}
            maxH={maxH}
            py={0.5}
          >
            <Box
              px={2}
              py={1}
              borderBottom='1px solid'
              borderBottomColor='app.hover'
            >
              <Flex
                justify='space-between'
                align='center'
                mb={searchable ? 1 : undefined}
              >
                <Text
                  fontSize='10px'
                  color='whiteAlpha.500'
                  fontWeight='medium'
                >
                  {title}
                </Text>
                {headerActions ??
                  (hasSelection && (
                    <LogFilterAction
                      label={`Clear (${selected.length})`}
                      onClick={handleClearAll}
                    />
                  ))}
              </Flex>
              {searchable && (
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
              )}
            </Box>
            <Box maxH='200px' overflowY='auto'>
              {visibleItems.length === 0 ? (
                <Box px={2} py={1}>
                  <Text fontSize='10px' color='whiteAlpha.400'>
                    {items.length === 0 ? emptyLabel : 'No match'}
                  </Text>
                </Box>
              ) : (
                visibleItems.map(item => (
                  <Menu.Item
                    key={item}
                    value={item}
                    onClick={() => handleToggle(item)}
                    bg='transparent'
                    _hover={{ bg: 'whiteAlpha.50' }}
                    py={1}
                    px={2}
                  >
                    <Flex align='center' gap={1.5} w='100%'>
                      <Checkbox.Root
                        checked={selected.includes(item)}
                        size='sm'
                      >
                        <Checkbox.HiddenInput />
                        <Checkbox.Control
                          borderColor='app.border'
                          _checked={{
                            bg: accent.checkedBg,
                            borderColor: accent.checkedBg,
                          }}
                        >
                          <Checkbox.Indicator />
                        </Checkbox.Control>
                      </Checkbox.Root>
                      {renderItem(item)}
                    </Flex>
                  </Menu.Item>
                ))
              )}
            </Box>
          </Menu.Content>
        </Menu.Positioner>
      </Portal>
    </Menu.Root>
  )
}
