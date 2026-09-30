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
        <Text fontSize='xs' color='whiteAlpha.400'>
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
        <Spinner size='sm' color='blue.400' />
        {isAll && (
          <Text fontSize='xs' color='whiteAlpha.500'>
            Loading contexts...
          </Text>
        )}
      </Flex>
    )
  }
  if (error) {
    return (
      <Flex align='center' justify='center' height='100%' minHeight='200px'>
        <Text fontSize='xs' color='red.300' textAlign='center'>
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
        <Text fontSize='xs' color='whiteAlpha.500' mb={1}>
          No resources
        </Text>
        <Text fontSize='xs' color='whiteAlpha.400'>
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
          <Spinner size='xs' color='blue.400' />
        </Flex>
      )}
    </Stack>
  )
}
