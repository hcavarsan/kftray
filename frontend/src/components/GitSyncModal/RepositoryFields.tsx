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
      <Text fontSize='xs' color='fg.muted'>
        GitHub Repository URL
      </Text>
      <Input
        value={repoUrl}
        onChange={(e: ChangeEvent<HTMLInputElement>) =>
          onRepoUrlChange(e.target.value)
        }
        placeholder='https://github.com/username/repo'
        bg='bg.surface'
        borderColor='border'
        position='relative'
        _hover={{
          borderColor: 'border.strong',
          bg: 'bg.surface',
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
