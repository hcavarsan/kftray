import { Flex, Spinner, Stack, Text } from '@chakra-ui/react'

import { errorMessage } from '@/lib/errors'

import { ResourceRow } from './ResourceRow'
import type { FlatResource } from './types'

interface ResourceListProps {
  hasSelection: boolean
  isAll: boolean
  isFetching: boolean
  error: Error | null | undefined
  resources: FlatResource[]
  deletingKey: string | undefined
  onDelete: (resource: FlatResource) => void
}

export function ResourceList({
  hasSelection,
  isAll,
  isFetching,
  error,
  resources,
  deletingKey,
  onDelete,
}: ResourceListProps) {
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
  if (error) {
    return (
      <Flex align='center' justify='center' height='100%' minHeight='200px'>
        <Text fontSize='xs' color='danger.fg' textAlign='center'>
          Failed to load resources: {errorMessage(error)}
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
      {resources.map(resource => (
        <ResourceRow
          key={resource.key}
          resource={resource}
          isDeleting={deletingKey === resource.key}
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
