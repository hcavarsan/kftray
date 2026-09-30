import { Flex, Spinner, Stack, Text } from '@chakra-ui/react'

import { errorMessage } from '@/lib/errors'

import { ResourceRow } from './ResourceRow'
import type { FlatResource } from './types'

interface ResourceListProps {
  hasSelection: boolean
  isAll: boolean
  isFetching: boolean
  error: Error | null | undefined
  failedContexts: string[]
  allFailed: boolean
  resources: FlatResource[]
  deletingKey: string | undefined
  deleteDisabled: boolean
  onDelete: (resource: FlatResource) => void
}

export function ResourceList({
  hasSelection,
  isAll,
  isFetching,
  error,
  failedContexts,
  allFailed,
  resources,
  deletingKey,
  deleteDisabled,
  onDelete,
}: ResourceListProps) {
  const failedNotice = failedContexts.length > 0 && (
    <Text fontSize='xs' color='danger.fg' textAlign='center' mb={2}>
      Failed to load: {failedContexts.join(', ')}
    </Text>
  )

  if (!hasSelection) {
    return (
      <Flex align='center' justify='center' height='100%' minHeight='200px'>
        <Text fontSize='xs' color='fg.faint'>
          Select a context
        </Text>
      </Flex>
    )
  }
  if (resources.length === 0 && isFetching) {
    return (
      <Flex
        justify='center'
        align='center'
        height='100%'
        minHeight='200px'
        direction='column'
        gap={2}
      >
        <Spinner size='sm' color='accent.fg' />
        {isAll && (
          <Text fontSize='xs' color='fg.subtle'>
            Loading contexts...
          </Text>
        )}
      </Flex>
    )
  }
  if (error || allFailed) {
    return (
      <Flex align='center' justify='center' height='100%' minHeight='200px'>
        <Text fontSize='xs' color='danger.fg' textAlign='center'>
          Failed to load resources:{' '}
          {error ? errorMessage(error) : failedContexts.join(', ')}
        </Text>
      </Flex>
    )
  }
  if (resources.length === 0) {
    return (
      <Flex
        direction='column'
        align='center'
        justify='center'
        height='100%'
        minHeight='200px'
      >
        {failedNotice}
        <Text fontSize='xs' color='fg.subtle' mb={1}>
          No resources
        </Text>
        <Text fontSize='xs' color='fg.faint'>
          Server pods appear when port forwards start
        </Text>
      </Flex>
    )
  }

  return (
    <Stack gap={2}>
      {failedNotice}
      {resources.map(resource => (
        <ResourceRow
          key={resource.key}
          resource={resource}
          isDeleting={deletingKey === resource.key}
          disabled={deleteDisabled}
          onDelete={onDelete}
        />
      ))}
      {isFetching && (
        <Flex justify='center' py={2}>
          <Spinner size='xs' color='accent.fg' />
        </Flex>
      )}
    </Stack>
  )
}
