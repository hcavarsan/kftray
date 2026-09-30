import { useState } from 'react'
import { AlertTriangle, Edit2, Plus, Trash2, Wrench } from 'lucide-react'

import {
  Box,
  Flex,
  HStack,
  Stack,
  Text,
  Wrap,
  WrapItem,
} from '@chakra-ui/react'
import { useQuery } from '@tanstack/react-query'

import { Button } from '@/components/ui/button'
import {
  AppDialog,
  AppDialogBody,
  AppDialogFooter,
  DialogCancelButton,
} from '@/components/ui/dialog'
import { configsQuery } from '@/hooks/useConfigs'
import { type Shortcut, useGlobalShortcuts } from '@/hooks/useGlobalShortcuts'
import { errorMessage } from '@/lib/errors'

import { findShortcutAction, shortcutConfigIds } from './actions'
import { ShortcutFormModal } from './ShortcutFormModal'

export function ShortcutModal({ onClose }: { onClose: () => void }) {
  const [editing, setEditing] = useState<Shortcut | 'new' | null>(null)
  const { data: configs = [] } = useQuery({
    ...configsQuery,
    meta: {
      errorToast: {
        title: 'Error',
        description: 'Failed to load configurations',
        duration: 3000,
      },
    },
  })
  const { shortcuts, platformStatus, deleteShortcut, fixPermissions } =
    useGlobalShortcuts()

  const shortcutList = shortcuts.data ?? []

  return (
    <>
      <AppDialog
        title='Global Shortcuts'
        onClose={onClose}
        maxWidth='600px'
        height='96vh'
      >
        <AppDialogBody overflowX='hidden'>
          {platformStatus?.platform === 'linux' &&
            platformStatus.needs_permission_fix && (
              <Box
                bg='bg.surface'
                p={3}
                borderRadius='md'
                border='1px solid'
                borderColor='warning.border'
                mb={3}
              >
                <Flex align='center' gap={2} mb={2}>
                  <AlertTriangle
                    size={14}
                    color='var(--chakra-colors-warning-fg)'
                  />
                  <Text
                    fontSize='xs'
                    fontWeight='medium'
                    color='warning.fg'
                    letterSpacing='0.025em'
                  >
                    Linux Permission Issue
                  </Text>
                </Flex>
                <Text
                  fontSize='xs'
                  color='fg.secondary'
                  lineHeight='1.4'
                  mb={2}
                >
                  Missing input group permissions. Shortcuts won&apos;t work
                  when app window is hidden.
                </Text>
                <Text fontSize='xs' color='fg.muted' lineHeight='1.4' mb={3}>
                  Current: {platformStatus.current_implementation}
                </Text>
                {platformStatus.can_fix_permissions && (
                  <Button
                    size='2xs'
                    variant='outline'
                    onClick={() => fixPermissions.mutate()}
                    loading={fixPermissions.isPending}
                    loadingText='Fixing...'
                    borderColor='warning.border'
                    color='warning.fg'
                    _hover={{
                      borderColor: 'warning.border',
                      bg: 'warning.subtle',
                      color: 'warning.fg',
                    }}
                    height='24px'
                    fontSize='xs'
                    px={3}
                  >
                    <Wrench size={10} />
                    <Text ml={1.5} fontSize='xs'>
                      Try Fix Permissions
                    </Text>
                  </Button>
                )}
              </Box>
            )}
          <Stack gap={2.5}>
            {shortcutList.length === 0 ? (
              <Box
                bg='bg.surface'
                p={3}
                borderRadius='md'
                border='1px solid'
                borderColor='border'
                textAlign='center'
              >
                <Text
                  fontSize='xs'
                  color='fg.secondary'
                  mb={1}
                  fontWeight='normal'
                  letterSpacing='0.025em'
                >
                  {shortcuts.isError
                    ? 'Failed to load shortcuts'
                    : 'No shortcuts configured'}
                </Text>
                <Text fontSize='xs' color='fg.muted' lineHeight='1.3'>
                  {shortcuts.isError
                    ? errorMessage(shortcuts.error)
                    : 'Add your first keyboard shortcut to get started'}
                </Text>
              </Box>
            ) : (
              shortcutList.map(shortcut => {
                const action = findShortcutAction(shortcut.action_type)
                const configIds = action?.requiresConfig
                  ? shortcutConfigIds(shortcut.action_data)
                  : []
                const relatedConfigs = configs.filter(config =>
                  configIds.includes(config.id),
                )

                return (
                  <Box
                    key={shortcut.id}
                    bg='bg.surface'
                    p={2}
                    borderRadius='md'
                    border='1px solid'
                    borderColor='border'
                    _hover={{ borderColor: 'border.emphasized' }}
                    display='flex'
                    flexDirection='column'
                    height='100%'
                  >
                    <Flex align='center' justify='space-between' mb={1}>
                      <Box flex={1}>
                        <Text
                          fontSize='xs'
                          fontWeight='normal'
                          color='fg.secondary'
                          mb={1}
                          letterSpacing='0.025em'
                        >
                          {action?.name ?? shortcut.action_type}
                        </Text>
                        <Text
                          fontSize='xs'
                          color='fg.muted'
                          fontFamily='mono'
                          bg='bg.hover'
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
                          aria-label='Edit shortcut'
                          size='2xs'
                          variant='ghost'
                          onClick={() => setEditing(shortcut)}
                          color='fg.muted'
                          _hover={{ color: 'fg', bg: 'bg.hover' }}
                          height='20px'
                          px={2}
                          minW='auto'
                        >
                          <Edit2 size={8} />
                        </Button>
                        <Button
                          aria-label='Delete shortcut'
                          size='2xs'
                          variant='ghost'
                          onClick={() => deleteShortcut.mutate(shortcut.id)}
                          disabled={deleteShortcut.isPending}
                          color='danger.fg'
                          _hover={{ color: 'danger.fg', bg: 'danger.subtle' }}
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
                          color='fg.muted'
                          mb={0.5}
                          lineHeight='1.3'
                        >
                          Configs:
                        </Text>
                        <Wrap gap={1}>
                          {relatedConfigs.map(config => (
                            <WrapItem key={config.id}>
                              <Box
                                bg='bg.faint'
                                border='1px solid'
                                borderColor='border.subtle'
                                borderRadius='sm'
                                px={2}
                                py={1}
                              >
                                <Text fontSize='xs' color='fg.muted'>
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
        </AppDialogBody>

        <AppDialogFooter justify='space-between'>
          <Button
            onClick={() => setEditing('new')}
            variant='ghost'
            size='xs'
            _hover={{ bg: 'bg.faint' }}
            color='fg.secondary'
            height='28px'
            fontSize='xs'
            px={2}
          >
            <Plus size={10} />
            <Text ml={1} fontSize='xs' fontWeight='normal'>
              Add New Shortcut
            </Text>
          </Button>

          <DialogCancelButton label='Close' onClick={onClose} />
        </AppDialogFooter>
      </AppDialog>

      {editing && (
        <ShortcutFormModal
          shortcut={editing === 'new' ? null : editing}
          configs={configs}
          onClose={() => setEditing(null)}
        />
      )}
    </>
  )
}
