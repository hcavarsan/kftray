import { Box, Dialog, Flex, HStack, Text } from '@chakra-ui/react'

import { Button } from '@/components/ui/button'
import { AppDialog } from '@/components/ui/dialog'

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
            css={{
              '&::-webkit-scrollbar': { width: '4px' },
              '&::-webkit-scrollbar-track': { background: 'transparent' },
              '&::-webkit-scrollbar-thumb': {
                background: 'var(--chakra-colors-app-border-strong)',
                borderRadius: '2px',
              },
            }}
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

      <Dialog.Footer
        p={2}
        borderTop='1px solid'
        borderColor='app.hover'
        bg='app.panel'
      >
        <HStack justify='flex-end' gap={2} width='100%'>
          <Button
            size='xs'
            variant='ghost'
            onClick={onClose}
            disabled={isPending}
            _hover={{ bg: 'app.hover' }}
            height='28px'
          >
            Cancel
          </Button>
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
        </HStack>
      </Dialog.Footer>
    </AppDialog>
  )
}
