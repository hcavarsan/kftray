import React, { useCallback, useState } from 'react'
import { Plus, X } from 'lucide-react'

import { Box, HStack, Input } from '@chakra-ui/react'

import { Button } from '@/components/ui/button'
import { Tooltip } from '@/components/ui/tooltip'
import { DEFAULT_CONFIG_TAB } from '@/types'

export interface ConfigTabsProps {
  tabs: string[]
  activeTab: string
  onSelectTab: (tab: string) => void
  onCreateTab: (name: string) => void
  onRenameTab: (from: string, to: string) => void
  onDeleteTab: (tab: string) => void
  tabHasConfigs: (tab: string) => boolean
}

const ConfigTabs: React.FC<ConfigTabsProps> = ({
  tabs,
  activeTab,
  onSelectTab,
  onCreateTab,
  onRenameTab,
  onDeleteTab,
  tabHasConfigs,
}) => {
  const [editingTab, setEditingTab] = useState<string | null>(null)
  const [editValue, setEditValue] = useState('')
  const [creating, setCreating] = useState(false)
  const [createValue, setCreateValue] = useState('')

  const commitRename = useCallback(() => {
    if (!editingTab) {
      return
    }
    const next = editValue.trim()
    if (next && next !== editingTab) {
      onRenameTab(editingTab, next)
    }
    setEditingTab(null)
    setEditValue('')
  }, [editingTab, editValue, onRenameTab])

  const commitCreate = useCallback(() => {
    const next = createValue.trim()
    if (next) {
      onCreateTab(next)
    }
    setCreating(false)
    setCreateValue('')
  }, [createValue, onCreateTab])

  return (
    <Box
      px={1}
      py={1}
      mb={1}
      borderBottom='1px solid rgba(255, 255, 255, 0.06)'
      overflowX='auto'
    >
      <HStack gap={1} align='center' flexWrap='nowrap'>
        {tabs.map(tab => {
          const isActive = tab === activeTab
          const isEditing = editingTab === tab

          if (isEditing) {
            return (
              <Input
                key={tab}
                size='xs'
                value={editValue}
                autoFocus
                maxW='120px'
                h='24px'
                fontSize='11px'
                bg='#1a1a1a'
                borderColor='whiteAlpha.200'
                onChange={e => setEditValue(e.target.value)}
                onBlur={commitRename}
                onKeyDown={e => {
                  if (e.key === 'Enter') {
                    commitRename()
                  }
                  if (e.key === 'Escape') {
                    setEditingTab(null)
                  }
                }}
              />
            )
          }

          return (
            <Box
              key={tab}
              display='flex'
              alignItems='center'
              gap={0.5}
              px={2}
              h='24px'
              borderRadius='md'
              fontSize='11px'
              cursor='pointer'
              bg={isActive ? 'whiteAlpha.200' : 'transparent'}
              color={isActive ? 'gray.100' : 'gray.400'}
              border='1px solid'
              borderColor={isActive ? 'whiteAlpha.300' : 'transparent'}
              _hover={{ bg: 'whiteAlpha.100', color: 'gray.100' }}
              onClick={() => onSelectTab(tab)}
              onDoubleClick={() => {
                setEditingTab(tab)
                setEditValue(tab)
              }}
              title='Double-click to rename'
            >
              <Box as='span' whiteSpace='nowrap' maxW='100px' truncate>
                {tab}
              </Box>
              {tab !== DEFAULT_CONFIG_TAB && (
                <Box
                  as='button'
                  display='flex'
                  alignItems='center'
                  ml={0.5}
                  p={0}
                  color='gray.500'
                  _hover={{ color: 'red.300' }}
                  onClick={e => {
                    e.stopPropagation()
                    if (tabHasConfigs(tab)) {
                      window.alert(
                        'Tab has configs. Move or delete them first.',
                      )
                      return
                    }
                    onDeleteTab(tab)
                  }}
                >
                  <X size={10} />
                </Box>
              )}
            </Box>
          )
        })}

        {creating ? (
          <Input
            size='xs'
            value={createValue}
            autoFocus
            maxW='120px'
            h='24px'
            fontSize='11px'
            placeholder='Tab name'
            bg='#1a1a1a'
            borderColor='whiteAlpha.200'
            onChange={e => setCreateValue(e.target.value)}
            onBlur={commitCreate}
            onKeyDown={e => {
              if (e.key === 'Enter') {
                commitCreate()
              }
              if (e.key === 'Escape') {
                setCreating(false)
                setCreateValue('')
              }
            }}
          />
        ) : (
          <Tooltip content='New tab' portalled>
            <Button
              size='xs'
              variant='ghost'
              h='24px'
              minW='24px'
              px={1}
              onClick={() => setCreating(true)}
            >
              <Plus size={12} />
            </Button>
          </Tooltip>
        )}
      </HStack>
    </Box>
  )
}

export default ConfigTabs
