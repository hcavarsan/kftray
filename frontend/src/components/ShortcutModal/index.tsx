import React, { useCallback, useEffect, useState } from 'react'
import { Edit2, Plus, Trash2 } from 'lucide-react'

import {
  Box,
  Dialog,
  Flex,
  HStack,
  Stack,
  Text,
  Wrap,
  WrapItem,
} from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import { DialogCloseTrigger } from '@/components/ui/dialog'
import { toaster } from '@/components/ui/toaster'
import { type Shortcut, useGlobalShortcuts } from '@/hooks/useGlobalShortcuts'
import { type Config } from '@/types'

import ShortcutFormModal from './ShortcutFormModal'

interface ShortcutModalProps {
  isOpen: boolean
  onClose: () => void
}

interface ShortcutAction {
  id: string
  name: string
  actionType: string
  requiresConfig: boolean
}

const SHORTCUT_ACTIONS: ShortcutAction[] = [
  {
    id: 'toggle_window',
    name: 'Toggle Window',
    actionType: 'toggle_window',
    requiresConfig: false,
  },
  {
    id: 'start_all',
    name: 'Start All Port Forward',
    actionType: 'start_all_port_forward',
    requiresConfig: false,
  },
  {
    id: 'stop_all',
    name: 'Stop All Port Forward',
    actionType: 'stop_all_port_forward',
    requiresConfig: false,
  },
  {
    id: 'start_port_forward',
    name: 'Start Port Forward',
    actionType: 'start_port_forward',
    requiresConfig: true,
  },
  {
    id: 'stop_port_forward',
    name: 'Stop Port Forward',
    actionType: 'stop_port_forward',
    requiresConfig: true,
  },
  {
    id: 'toggle_port_forward',
    name: 'Toggle Port Forward',
    actionType: 'toggle_port_forward',
    requiresConfig: true,
  },
]

const ShortcutModal: React.FC<ShortcutModalProps> = ({ isOpen, onClose }) => {
  const [configs, setConfigs] = useState<Config[]>([])
  const [editingShortcut, setEditingShortcut] = useState<Shortcut | null>(null)
  const [isFormModalOpen, setIsFormModalOpen] = useState(false)

  const { shortcuts, deleteShortcut, refreshShortcuts } = useGlobalShortcuts()

  useEffect(() => {
    if (isOpen) {
      loadConfigs()
      refreshShortcuts()
    }
  }, [isOpen, refreshShortcuts])

  const loadConfigs = async () => {
    try {
      const allConfigs = await invoke<Config[]>('get_configs_cmd')

      setConfigs(allConfigs)
    } catch (error) {
      console.error('Failed to load configs:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to load configurations',
        duration: 3000,
      })
    }
  }

  const handleFormSaved = async () => {
    setIsFormModalOpen(false)
    setEditingShortcut(null)
    await refreshShortcuts()
  }

  const handleDeleteShortcut = async (id: number) => {
    try {
      const deleted = await deleteShortcut(id)

      if (deleted) {
        toaster.success({
          title: 'Deleted',
          description: 'Shortcut deleted successfully',
          duration: 3000,
        })
      }
    } catch (error) {
      console.error('Error deleting shortcut:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to delete shortcut',
        duration: 3000,
      })
    }
  }

  const handleEditShortcut = (shortcut: Shortcut) => {
    setEditingShortcut(shortcut)
    setIsFormModalOpen(true)
  }

  const handleAddShortcut = () => {
    setEditingShortcut(null)
    setIsFormModalOpen(true)
  }

  const getShortcutDisplayInfo = useCallback(
    (shortcut: Shortcut) => {
      const action = SHORTCUT_ACTIONS.find(
        a => a.actionType === shortcut.action_type,
      )

      return {
        actionName: action?.name || shortcut.action_type,
        relatedConfigs:
          action?.requiresConfig && shortcut.action_data
            ? (() => {
                try {
                  const data = JSON.parse(shortcut.action_data)
                  const configIds = data.config_ids || []

                  return configs.filter(c => configIds.includes(c.id))
                } catch (e) {
                  console.error('Error parsing action data:', e)

                  return []
                }
              })()
            : [],
      }
    },
    [configs],
  )

  return (
    <Dialog.Root
      open={isOpen}
      onOpenChange={({ open }) => !open && onClose()}
      modal={true}
      closeOnEscape={true}
    >
      <Dialog.Backdrop
        bg='transparent'
        backdropFilter='blur(4px)'
        height='100vh'
      />
      <Dialog.Positioner overflow='hidden'>
        <Dialog.Content
          onClick={e => e.stopPropagation()}
          maxWidth='600px'
          width='90vw'
          height='96vh'
          bg='#111111'
          borderRadius='lg'
          border='1px solid rgba(255, 255, 255, 0.08)'
          overflow='hidden'
          position='absolute'
          my={2}
          display='flex'
          flexDirection='column'
        >
          <DialogCloseTrigger style={{ marginTop: '-4px' }} />

          <Dialog.Header
            p={3}
            bg='#161616'
            borderBottom='1px solid rgba(255, 255, 255, 0.05)'
          >
            <Text fontSize='sm' fontWeight='medium' color='gray.100'>
              Global Shortcuts
            </Text>
          </Dialog.Header>

          <Dialog.Body p={3} flex={1} overflowY='auto' overflowX='hidden'>
            <Stack gap={2.5}>
              {shortcuts.length === 0 ? (
                <Box
                  bg='#161616'
                  p={3}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                  textAlign='center'
                >
                  <Text
                    fontSize='xs'
                    color='gray.300'
                    mb={1}
                    fontWeight='normal'
                    letterSpacing='0.025em'
                  >
                    No shortcuts configured
                  </Text>
                  <Text fontSize='xs' color='gray.400' lineHeight='1.3'>
                    Add your first keyboard shortcut to get started
                  </Text>
                </Box>
              ) : (
                shortcuts.map(shortcut => {
                  const { actionName, relatedConfigs } =
                    getShortcutDisplayInfo(shortcut)

                  return (
                    <Box
                      key={shortcut.id}
                      bg='#161616'
                      p={2}
                      borderRadius='md'
                      border='1px solid rgba(255, 255, 255, 0.08)'
                      _hover={{
                        borderColor: 'rgba(255, 255, 255, 0.15)',
                      }}
                      display='flex'
                      flexDirection='column'
                      height='100%'
                    >
                      <Flex align='center' justify='space-between' mb={1}>
                        <Box flex={1}>
                          <Text
                            fontSize='xs'
                            fontWeight='normal'
                            color='gray.300'
                            mb={1}
                            letterSpacing='0.025em'
                          >
                            {actionName}
                          </Text>
                          <Text
                            fontSize='xs'
                            color='gray.400'
                            fontFamily='mono'
                            bg='rgba(255, 255, 255, 0.05)'
                            px={2}
                            py={1}
                            borderRadius='sm'
                            display='inline-block'
                          >
                            {shortcut.shortcut_key}
                          </Text>
                        </Box>
                        <HStack gap={1}>
                          <Button
                            size='2xs'
                            variant='ghost'
                            onClick={() => handleEditShortcut(shortcut)}
                            color='whiteAlpha.700'
                            _hover={{ color: 'white', bg: 'whiteAlpha.100' }}
                            height='20px'
                            px={2}
                            minW='auto'
                          >
                            <Edit2 size={8} />
                          </Button>
                          <Button
                            size='2xs'
                            variant='ghost'
                            onClick={() => handleDeleteShortcut(shortcut.id)}
                            color='red.300'
                            _hover={{ color: 'red.200', bg: 'red.900' }}
                            height='20px'
                            px={2}
                            minW='auto'
                          >
                            <Trash2 size={8} />
                          </Button>
                        </HStack>
                      </Flex>

                      {relatedConfigs.length > 0 && (
                        <Box flex='1'>
                          <Text
                            fontSize='xs'
                            color='gray.400'
                            mb={0.5}
                            lineHeight='1.3'
                          >
                            Configs:
                          </Text>
                          <Wrap gap={1}>
                            {relatedConfigs.map(config => (
                              <WrapItem key={config.id}>
                                <Box
                                  bg='rgba(255, 255, 255, 0.03)'
                                  border='1px solid rgba(255, 255, 255, 0.05)'
                                  borderRadius='sm'
                                  px={2}
                                  py={1}
                                >
                                  <Text fontSize='xs' color='gray.400'>
                                    {config.alias}
                                  </Text>
                                </Box>
                              </WrapItem>
                            ))}
                          </Wrap>
                        </Box>
                      )}
                    </Box>
                  )
                })
              )}
            </Stack>
          </Dialog.Body>

          <Dialog.Footer
            px={3}
            py={2}
            bg='#161616'
            borderTop='1px solid rgba(255, 255, 255, 0.05)'
            flexShrink={0}
          >
            <Flex justify='space-between' align='center' width='100%'>
              <Button
                onClick={handleAddShortcut}
                variant='ghost'
                size='xs'
                _hover={{ bg: 'whiteAlpha.50' }}
                color='gray.300'
                height='28px'
                fontSize='xs'
                px={2}
              >
                <Plus size={10} />
                <Text ml={1} fontSize='xs' fontWeight='normal'>
                  Add New Shortcut
                </Text>
              </Button>

              <Button
                variant='ghost'
                size='xs'
                onClick={onClose}
                _hover={{ bg: 'whiteAlpha.50' }}
                color='gray.400'
                height='28px'
                fontSize='xs'
              >
                Close
              </Button>
            </Flex>
          </Dialog.Footer>
        </Dialog.Content>
      </Dialog.Positioner>

      <ShortcutFormModal
        isOpen={isFormModalOpen}
        onClose={() => setIsFormModalOpen(false)}
        editingShortcut={editingShortcut}
        configs={configs}
        onSaved={handleFormSaved}
      />
    </Dialog.Root>
  )
}

export default ShortcutModal
