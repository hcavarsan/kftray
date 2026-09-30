import { Box, Dialog, Flex, Text } from '@chakra-ui/react'

import { Button } from '@/components/ui/button'
import {
  AppDialog,
  AppDialogFooter,
  DialogCancelButton,
} from '@/components/ui/dialog'

import type { CleanupMode, FlatResource } from './types'

interface CleanupDialogProps {
  mode: CleanupMode
  resources: FlatResource[]
  scope: string
  isPending: boolean
  onClose: () => void
  onConfirm: () => void
}

export function CleanupDialog({
  mode,
  resources,
  scope,
  isPending,
  onClose,
  onConfirm,
}: CleanupDialogProps) {
  const count = resources.length
  const title =
    mode === 'orphaned' ? 'Clean Orphaned Resources' : 'Delete All Resources'
  const noun = `${mode === 'orphaned' ? 'orphaned ' : ''}${count === 1 ? 'resource' : 'resources'}`

  return (
    <AppDialog
      title={title}
      onClose={onClose}
      closeDisabled={isPending}
      role='alertdialog'
      placement='center'
      maxWidth='420px'
    >
      <Dialog.Body p={3}>
        <Text fontSize='xs' color='whiteAlpha.700' lineHeight='1.5' mb={3}>
          Delete {count} {noun} {scope}?
          {mode === 'all' && (
            <Text as='span' color='orange.400' fontWeight='500'>
              {' '}
              This will also stop active port forwards.
            </Text>
          )}
        </Text>

        {count > 0 && (
          <Box
            bg='app.deep'
            borderRadius='md'
            border='1px solid'
            borderColor='app.hover'
            maxHeight='200px'
            overflowY='auto'
          >
            {resources.map(resource => (
              <Box
                key={resource.key}
                px={2}
                py={1.5}
                borderBottom='1px solid'
                borderColor='app.faint'
                _last={{ borderBottom: 'none' }}
              >
                <Text fontSize='xs' color='whiteAlpha.800' truncate>
                  {resource.name}
                </Text>
                <Flex gap={1} fontSize='10px' color='whiteAlpha.500' mt={0.5}>
                  <Text>{resource.resource_type}</Text>
                  <Text color='whiteAlpha.300'>·</Text>
                  <Text truncate>{resource.context}</Text>
                  <Text color='whiteAlpha.300'>/</Text>
                  <Text truncate>{resource.namespace}</Text>
                </Flex>
              </Box>
            ))}
          </Box>
        )}
      </Dialog.Body>

      <AppDialogFooter>
        <DialogCancelButton onClick={onClose} disabled={isPending} />
        <Button
          size='xs'
          colorPalette='red'
          onClick={onConfirm}
          loading={isPending}
          loadingText='Deleting...'
          height='28px'
        >
          Delete {count}
        </Button>
      </AppDialogFooter>
    </AppDialog>
  )
}
