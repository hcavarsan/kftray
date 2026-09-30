import type { ChangeEvent } from 'react'
import { Plus, X } from 'lucide-react'

import {
  Box,
  Flex,
  HStack,
  IconButton,
  Input,
  Stack,
  Text,
} from '@chakra-ui/react'

interface ConfigPathEntry {
  id: string
  value: string
}

interface ConfigPathsFieldProps {
  configPaths: ConfigPathEntry[]
  onChange: (id: string, value: string) => void
  onAdd: () => void
  onRemove: (id: string) => void
}

export function ConfigPathsField({
  configPaths,
  onChange,
  onAdd,
  onRemove,
}: ConfigPathsFieldProps) {
  return (
    <Stack gap={2}>
      <Flex justify='space-between' align='center'>
        <Text fontSize='xs' color='gray.400'>
          Config Path(s)
        </Text>
        <IconButton
          aria-label='Add config path'
          size='xs'
          variant='ghost'
          onClick={onAdd}
          color='gray.400'
          _hover={{ bg: 'whiteAlpha.100' }}
        >
          <Box as={Plus} width='12px' height='12px' />
        </IconButton>
      </Flex>
      {configPaths.map(field => (
        <HStack key={field.id} gap={1}>
          <Input
            value={field.value}
            onChange={(e: ChangeEvent<HTMLInputElement>) =>
              onChange(field.id, e.target.value)
            }
            placeholder='path/to/config.json'
            bg='app.panel'
            borderColor='app.border'
            _hover={{
              borderColor: 'app.divider',
              bg: 'app.panel',
            }}
            height='30px'
            fontSize='12px'
          />
          <IconButton
            aria-label='Remove config path'
            size='xs'
            variant='ghost'
            onClick={() => onRemove(field.id)}
            color='gray.500'
            _hover={{ bg: 'whiteAlpha.100' }}
            disabled={configPaths.length === 1 && !field.value}
          >
            <Box as={X} width='12px' height='12px' />
          </IconButton>
        </HStack>
      ))}
    </Stack>
  )
}
