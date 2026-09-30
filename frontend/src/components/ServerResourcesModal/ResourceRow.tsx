import {
  Box as BoxIcon,
  Database,
  GitBranch,
  Server,
  Trash2,
} from 'lucide-react'

import { Badge, Box, Flex, Spinner, Text } from '@chakra-ui/react'

import { Button } from '@/components/ui/button'
import { Tooltip } from '@/components/ui/tooltip'

import type { FlatResource } from './types'

interface ResourceRowProps {
  resource: FlatResource
  isDeleting: boolean
  onDelete: (resource: FlatResource) => void
}

const RESOURCE_ICONS: Record<string, typeof Server> = {
  pod: BoxIcon,
  deployment: Server,
  service: GitBranch,
  ingress: Database,
}

export function ResourceRow({
  resource,
  isDeleting,
  onDelete,
}: ResourceRowProps) {
  const Icon = RESOURCE_ICONS[resource.resource_type] ?? Server

  return (
    <Box
      bg='app.panel'
      p={2}
      borderRadius='md'
      border='1px solid'
      borderColor='white/4'
      _hover={{ borderColor: 'app.border' }}
    >
      <Flex align='center' gap={2} mb={1.5}>
        <Box color='whiteAlpha.500' flexShrink={0}>
          <Icon size={12} />
        </Box>

        <Tooltip
          content={resource.name}
          portalled
          positioning={{ placement: 'top' }}
        >
          <Text
            fontSize='xs'
            fontWeight='500'
            color='white'
            flex='1'
            truncate
            cursor='default'
          >
            {resource.name}
          </Text>
        </Tooltip>

        <Badge
          size='xs'
          colorPalette={resource.is_orphaned ? 'red' : 'gray'}
          variant='subtle'
          flexShrink={0}
        >
          {resource.is_orphaned ? 'orphaned' : 'active'}
        </Badge>

        <Button
          aria-label={`Delete ${resource.name}`}
          size='2xs'
          variant='ghost'
          onClick={() => onDelete(resource)}
          disabled={isDeleting}
          flexShrink={0}
          px={1}
          opacity={0.5}
          _hover={{ opacity: 1, color: 'red.400' }}
        >
          {isDeleting ? <Spinner size='xs' /> : <Trash2 size={11} />}
        </Button>
      </Flex>

      <Flex align='center' gap={1.5} fontSize='xs' color='whiteAlpha.500'>
        <Tooltip
          content={resource.context}
          portalled
          positioning={{ placement: 'top' }}
        >
          <Text truncate maxWidth='120px' cursor='default'>
            {resource.context}
          </Text>
        </Tooltip>

        <Text color='whiteAlpha.300'>/</Text>

        <Tooltip
          content={resource.namespace}
          portalled
          positioning={{ placement: 'top' }}
        >
          <Text truncate maxWidth='100px' cursor='default'>
            {resource.namespace}
          </Text>
        </Tooltip>

        <Text color='whiteAlpha.300' flexShrink={0}>
          ·
        </Text>

        <Text flexShrink={0}>{resource.resource_type}</Text>

        <Text color='whiteAlpha.300' flexShrink={0}>
          ·
        </Text>

        <Text flexShrink={0}>{resource.age}</Text>
      </Flex>
    </Box>
  )
}
