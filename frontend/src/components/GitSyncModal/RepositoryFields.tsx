import type { ChangeEvent } from 'react'

import { Input, Stack, Text } from '@chakra-ui/react'

interface RepositoryFieldsProps {
  repoUrl: string
  onRepoUrlChange: (value: string) => void
}

export function RepositoryFields({
  repoUrl,
  onRepoUrlChange,
}: RepositoryFieldsProps) {
  return (
    <Stack gap={2}>
      <Text fontSize='xs' color='gray.400'>
        GitHub Repository URL
      </Text>
      <Input
        value={repoUrl}
        onChange={(e: ChangeEvent<HTMLInputElement>) =>
          onRepoUrlChange(e.target.value)
        }
        placeholder='https://github.com/username/repo'
        bg='app.panel'
        borderColor='app.border'
        position='relative'
        _hover={{
          borderColor: 'app.divider',
          bg: 'app.panel',
          zIndex: 2,
        }}
        height='30px'
        fontSize='12px'
        borderRadius='md'
        px={2}
      />
    </Stack>
  )
}
